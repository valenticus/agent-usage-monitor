use std::io;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_util::sync::CancellationToken;

use crate::claude_status_line::ClaudeStatusLineSnapshot;

pub const CLAUDE_PIPE_NAME: &str = r"\\.\pipe\AgentMonitor.ClaudeStatusLine.v1";
pub const MAX_PIPE_MESSAGE_BYTES: usize = 2_048;
const MAX_CLIENT_IO_TIME: Duration = Duration::from_secs(2);
const CLIENT_CONNECT_TIME: Duration = Duration::from_millis(750);

pub fn parse_pipe_message(
    bytes: &[u8],
    now: i64,
) -> Result<ClaudeStatusLineSnapshot, &'static str> {
    if bytes.is_empty() || bytes.len() > MAX_PIPE_MESSAGE_BYTES {
        return Err("message size is invalid");
    }
    let snapshot: ClaudeStatusLineSnapshot =
        serde_json::from_slice(bytes).map_err(|_| "message data is invalid")?;
    snapshot
        .validate(now)
        .map_err(|_| "snapshot is outside allowed bounds")?;
    Ok(snapshot)
}

#[cfg(windows)]
pub async fn run_claude_status_pipe(
    updates: mpsc::Sender<ClaudeStatusLineSnapshot>,
    cancellation: CancellationToken,
) -> io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut first_instance = true;
    loop {
        if cancellation.is_cancelled() {
            return Ok(());
        }
        let mut server = loop {
            match create_pipe_server(first_instance) {
                Ok(server) => break server,
                Err(error) if !first_instance && error.raw_os_error() == Some(231) => {
                    tokio::select! {
                        _ = cancellation.cancelled() => return Ok(()),
                        _ = tokio::time::sleep(Duration::from_millis(25)) => {}
                    }
                }
                Err(error) => return Err(error),
            }
        };
        first_instance = false;

        let connected = tokio::select! {
            _ = cancellation.cancelled() => return Ok(()),
            result = server.connect() => result,
        };
        if connected.is_err() {
            let _ = server.disconnect();
            continue;
        }

        let result = timeout(MAX_CLIENT_IO_TIME, async {
            let mut length = [0u8; 4];
            server.read_exact(&mut length).await?;
            let length = u32::from_le_bytes(length) as usize;
            if length == 0 || length > MAX_PIPE_MESSAGE_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid frame size",
                ));
            }
            let mut bytes = vec![0u8; length];
            server.read_exact(&mut bytes).await?;
            let snapshot = parse_pipe_message(&bytes, now_unix_seconds())
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid snapshot"))?;
            updates
                .send(snapshot.clone())
                .await
                .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "receiver closed"))?;
            server.write_all(&[1]).await?;
            server.flush().await?;
            Ok::<(), io::Error>(())
        })
        .await;

        if result.is_err() {
            let _ = server.write_all(&[0]).await;
            let _ = server.flush().await;
        }
        let _ = server.disconnect();
        drop(server);
    }
}

#[cfg(windows)]
fn create_pipe_server(
    first_instance: bool,
) -> io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let (descriptor, mut attributes) = current_user_security_attributes()?;
    let mut options = ServerOptions::new();
    options
        .first_pipe_instance(first_instance)
        .max_instances(1)
        .reject_remote_clients(true)
        .access_inbound(true)
        .access_outbound(true)
        .in_buffer_size(MAX_PIPE_MESSAGE_BYTES as u32 + 4)
        .out_buffer_size(1);
    // SAFETY: the security descriptor and SECURITY_ATTRIBUTES remain alive
    // through CreateNamedPipeW; Windows copies the descriptor at creation.
    let server = unsafe {
        options.create_with_security_attributes_raw(
            CLAUDE_PIPE_NAME,
            (&mut attributes as *mut windows_sys::Win32::Security::SECURITY_ATTRIBUTES).cast(),
        )
    }?;
    drop(descriptor);
    Ok(server)
}

#[cfg(not(windows))]
pub async fn run_claude_status_pipe(
    _updates: mpsc::Sender<ClaudeStatusLineSnapshot>,
    _cancellation: CancellationToken,
) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Claude named pipes require Windows",
    ))
}

#[cfg(windows)]
pub async fn send_claude_status_snapshot(snapshot: &ClaudeStatusLineSnapshot) -> io::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::windows::named_pipe::ClientOptions;

    snapshot
        .validate(now_unix_seconds())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid status-line data"))?;
    let bytes = serde_json::to_vec(snapshot)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "status-line encoding failed"))?;
    if bytes.is_empty() || bytes.len() > MAX_PIPE_MESSAGE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "message too large",
        ));
    }
    let mut pipe = timeout(CLIENT_CONNECT_TIME, async {
        ClientOptions::new().open(CLAUDE_PIPE_NAME)
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "pipe connection timed out"))??;
    pipe.write_all(&(bytes.len() as u32).to_le_bytes()).await?;
    pipe.write_all(&bytes).await?;
    pipe.flush().await?;
    let mut acknowledgement = [0u8; 1];
    timeout(MAX_CLIENT_IO_TIME, pipe.read_exact(&mut acknowledgement))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "pipe response timed out"))??;
    if acknowledgement[0] != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "pipe rejected snapshot",
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
pub async fn send_claude_status_snapshot(_snapshot: &ClaudeStatusLineSnapshot) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Claude named pipes require Windows",
    ))
}

#[cfg(windows)]
fn current_user_security_attributes() -> io::Result<(
    LocalSecurityDescriptor,
    windows_sys::Win32::Security::SECURITY_ATTRIBUTES,
)> {
    use std::mem::size_of;
    use std::ptr::null_mut;

    use windows_sys::Win32::Foundation::{CloseHandle, LocalFree};
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{
        GetTokenInformation, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token = null_mut();
    // SAFETY: OpenProcessToken writes a process token handle to `token`.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let mut needed = 0u32;
    // SAFETY: the first query uses a null buffer to get the required size.
    unsafe {
        GetTokenInformation(token, TokenUser, null_mut(), 0, &mut needed);
    }
    if needed == 0 || needed > 16_384 {
        // SAFETY: token is a valid handle returned by OpenProcessToken.
        unsafe { CloseHandle(token) };
        return Err(io::Error::last_os_error());
    }
    let mut aligned = vec![0usize; (needed as usize).div_ceil(size_of::<usize>())];
    // SAFETY: `aligned` has sufficient size and suitable alignment for TOKEN_USER.
    if unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            aligned.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        unsafe { CloseHandle(token) };
        return Err(io::Error::last_os_error());
    }
    // SAFETY: GetTokenInformation populated the buffer with a TOKEN_USER record.
    let user = unsafe { &*(aligned.as_ptr().cast::<TOKEN_USER>()) };
    let mut sid_text = null_mut();
    // SAFETY: user.User.Sid came from the current process token and is valid.
    if unsafe { ConvertSidToStringSidW(user.User.Sid, &mut sid_text) } == 0 {
        unsafe { CloseHandle(token) };
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the returned SID string is null terminated and LocalFree-owned.
    let sid = unsafe {
        let mut length = 0usize;
        while *sid_text.add(length) != 0 {
            length += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(sid_text, length))
    };
    unsafe { LocalFree(sid_text.cast()) };
    unsafe { CloseHandle(token) };

    let sddl: Vec<u16> = format!("O:{sid}D:P(A;;GA;;;{sid})\0")
        .encode_utf16()
        .collect();
    let mut descriptor = null_mut();
    // SAFETY: sddl is a null-terminated SDDL string and descriptor is writable.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let security_descriptor = LocalSecurityDescriptor(descriptor);
    let attributes = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    Ok((security_descriptor, attributes))
}

#[cfg(windows)]
struct LocalSecurityDescriptor(*mut core::ffi::c_void);

#[cfg(windows)]
impl Drop for LocalSecurityDescriptor {
    fn drop(&mut self) {
        // SAFETY: the security descriptor was allocated by the Windows SDDL API.
        unsafe { windows_sys::Win32::Foundation::LocalFree(self.0) };
    }
}

fn now_unix_seconds() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{MAX_PIPE_MESSAGE_BYTES, parse_pipe_message};

    #[test]
    fn accepts_only_bounded_recent_normalized_snapshots() {
        let message = br#"{"five_hour_remaining_percent":73.5,"five_hour_reset_at":1700000100,"weekly_remaining_percent":null,"weekly_reset_at":null,"snapshot_at":1700000000}"#;
        assert!(parse_pipe_message(message, 1_700_000_001).is_ok());
        assert!(parse_pipe_message(&[], 1_700_000_001).is_err());
        assert!(
            parse_pipe_message(&vec![b' '; MAX_PIPE_MESSAGE_BYTES + 1], 1_700_000_001).is_err()
        );
        assert!(parse_pipe_message(
            br#"{"five_hour_remaining_percent":73.5,"five_hour_reset_at":1700000100,"weekly_remaining_percent":null,"weekly_reset_at":null,"snapshot_at":1700000000,"session_id":"not-allowed"}"#,
            1_700_000_001,
        )
        .is_err());
    }

    #[test]
    fn rejects_out_of_range_percentages_and_future_reset_timestamps() {
        for message in [
            br#"{"five_hour_remaining_percent":101,"five_hour_reset_at":null,"weekly_remaining_percent":null,"weekly_reset_at":null,"snapshot_at":1700000000}"#.as_slice(),
            br#"{"five_hour_remaining_percent":null,"five_hour_reset_at":1703000000,"weekly_remaining_percent":null,"weekly_reset_at":null,"snapshot_at":1700000000}"#.as_slice(),
        ] {
            assert!(parse_pipe_message(message, 1_700_000_001).is_err());
        }
    }
}
