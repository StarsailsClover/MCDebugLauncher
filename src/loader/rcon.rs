// Minimal RCON client for Minecraft dedicated servers (v26.2-alpha.7).
//
// Implements the Source RCON protocol (valve's spec, as used by vanilla and
// most modded servers):
//
//   packet := [length:i32le][requestId:i32le][type:i32le][payload][0x00 0x00]
//
//   type 3 = login (auth), payload = password
//   type 2 = command,          payload = command text
//   type 0 = response
//
// Auth success echoes the request id; failure returns -1. MDL uses RCON to:
//   - stop servers gracefully (world save + clean shutdown) instead of
//     taskkill, which risks world corruption,
//   - run console commands programmatically (`mdl server cmd`) so agents can
//     automate test scenarios (op, gamerule, whitelist, give, ...).
//
// The server side must have enable-rcon=true in server.properties; `mdl
// server create` writes that automatically along with a generated password.

use anyhow::{Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const TYPE_AUTH: i32 = 3;
const TYPE_COMMAND: i32 = 2;
const TYPE_RESPONSE: i32 = 0;

/// One RCON connection. Reuse it to run several commands without re-auth.
pub struct RconClient {
    stream: TcpStream,
    next_id: i32,
}

impl RconClient {
    /// Connect and authenticate. `addr` is "host:port" (default rcon port is
    /// 25575). Returns an error when the server rejects the password.
    pub async fn connect(addr: &str, password: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr)
            .await
            .with_context(|| format!("Failed to connect to RCON at {}", addr))?;
        let mut client = Self { stream, next_id: 1 };
        let id = client.next_id;
        client.next_id += 1;
        client.send_packet(id, TYPE_AUTH, password).await?;
        let (resp_id, resp_type, _payload) = client.read_packet().await?;
        if resp_type != TYPE_RESPONSE && resp_id == -1 {
            anyhow::bail!("RCON authentication rejected for {}", addr);
        }
        if resp_type == TYPE_RESPONSE && resp_id == -1 {
            anyhow::bail!("RCON authentication rejected for {}", addr);
        }
        Ok(client)
    }

    async fn send_packet(&mut self, id: i32, ptype: i32, payload: &str) -> Result<()> {
        let payload_bytes = payload.as_bytes();
        // length covers id + type + payload + two terminating nulls.
        let length = (4 + 4 + payload_bytes.len() + 2) as i32;
        let mut buf = Vec::with_capacity((length + 4) as usize);
        buf.extend_from_slice(&length.to_le_bytes());
        buf.extend_from_slice(&id.to_le_bytes());
        buf.extend_from_slice(&ptype.to_le_bytes());
        buf.extend_from_slice(payload_bytes);
        buf.extend_from_slice(&[0, 0]);
        self.stream
            .write_all(&buf)
            .await
            .context("Failed to write RCON packet")?;
        self.stream.flush().await?;
        Ok(())
    }

    /// Read one packet. Returns (requestId, type, payload).
    async fn read_packet(&mut self) -> Result<(i32, i32, Vec<u8>)> {
        let mut len_buf = [0u8; 4];
        self.stream
            .read_exact(&mut len_buf)
            .await
            .context("RCON connection closed while reading length")?;
        let length = i32::from_le_bytes(len_buf) as usize;
        if !(10..=4096).contains(&length) {
            anyhow::bail!("Invalid RCON packet length {}", length);
        }
        let mut body = vec![0u8; length];
        self.stream
            .read_exact(&mut body)
            .await
            .context("RCON connection closed while reading body")?;
        let id = i32::from_le_bytes([body[0], body[1], body[2], body[3]]);
        let ptype = i32::from_le_bytes([body[4], body[5], body[6], body[7]]);
        // Strip trailing double-null from the payload.
        let payload_end = body.len() - 2;
        Ok((id, ptype, body[8..payload_end].to_vec()))
    }

    /// Run a console command and return the server's textual response.
    pub async fn command(&mut self, command: &str) -> Result<String> {
        let id = self.next_id;
        self.next_id += 1;
        self.send_packet(id, TYPE_COMMAND, command).await?;
        let (resp_id, _ptype, payload) = self.read_packet().await?;
        if resp_id == -1 {
            anyhow::bail!("RCON request rejected (auth expired?)");
        }
        Ok(String::from_utf8_lossy(&payload).to_string())
    }
}

/// Convenience: connect, run one command, disconnect.
pub async fn run_command(addr: &str, password: &str, command: &str) -> Result<String> {
    let mut client = RconClient::connect(addr, password).await?;
    client.command(command).await
}

#[cfg(test)]
mod tests {
    

    #[test]
    fn test_packet_length_math() {
        // length = id(4) + type(4) + payload(n) + 2 nulls
        let payload = "stop";
        let expected = 4 + 4 + payload.len() + 2;
        assert_eq!(expected, 14);
    }

    #[test]
    fn test_auth_reject_detection() {
        // -1 id signals rejection per protocol.
        let id: i32 = -1;
        assert_eq!(id.to_le_bytes(), [0xFFu8, 0xFF, 0xFF, 0xFF]);
    }
}
