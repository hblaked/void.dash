//! Minecraft Java status and Source RCON, with bounded frames and I/O deadlines.
use crate::model::Player;
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};

type Result<T> = std::result::Result<T, String>;

pub struct Status {
    pub motd: String,
    pub icon: Option<String>,
    pub version: String,
    pub online: u32,
    pub max: u32,
    pub players: Vec<Player>,
    pub latency: u64,
}

fn varint(mut n: i32) -> Vec<u8> {
    let mut out = vec![];
    loop {
        let mut byte = (n & 0x7f) as u8;
        n = ((n as u32) >> 7) as i32;
        if n != 0 {
            byte |= 0x80;
        }
        out.push(byte);
        if n == 0 {
            return out;
        }
    }
}
async fn read_varint(stream: &mut TcpStream) -> Result<i32> {
    let mut n = 0;
    for shift in (0..35).step_by(7) {
        let b = stream.read_u8().await.map_err(|e| e.to_string())?;
        n |= ((b & 0x7f) as i32) << shift;
        if b & 0x80 == 0 {
            return Ok(n);
        }
    }
    Err("Invalid VarInt".into())
}
fn take_varint(bytes: &[u8], offset: &mut usize) -> Result<i32> {
    let mut n = 0;
    for shift in (0..35).step_by(7) {
        let b = *bytes.get(*offset).ok_or("Truncated frame")?;
        *offset += 1;
        n |= ((b & 0x7f) as i32) << shift;
        if b & 0x80 == 0 {
            return Ok(n);
        }
    }
    Err("Invalid VarInt".into())
}
async fn send_frame(stream: &mut TcpStream, body: &[u8]) -> Result<()> {
    stream
        .write_all(&varint(body.len() as i32))
        .await
        .map_err(|e| e.to_string())?;
    stream.write_all(body).await.map_err(|e| e.to_string())
}
pub fn plain_chat(v: &Value) -> String {
    match v {
        Value::String(s) => strip_format(s),
        Value::Array(a) => a.iter().map(plain_chat).collect(),
        Value::Object(o) => {
            let mut s = o
                .get("text")
                .or_else(|| o.get("translate"))
                .map(plain_chat)
                .unwrap_or_default();
            if let Some(extra) = o.get("extra") {
                s += &plain_chat(extra);
            }
            s
        }
        _ => String::new(),
    }
}
pub fn strip_format(s: &str) -> String {
    let mut chars = s.chars();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        if c == '§' {
            chars.next();
        } else {
            out.push(c);
        }
    }
    out
}
pub async fn status(host: &str, port: u16) -> Result<Status> {
    timeout(Duration::from_secs(3), async {
        let start = Instant::now();
        let mut stream = TcpStream::connect((host, port))
            .await
            .map_err(|e| e.to_string())?;
        let mut handshake = vec![0];
        handshake.extend(varint(767));
        handshake.extend(varint(host.len() as i32));
        handshake.extend(host.as_bytes());
        handshake.extend(port.to_be_bytes());
        handshake.push(1);
        send_frame(&mut stream, &handshake).await?;
        send_frame(&mut stream, &[0]).await?;
        let len = read_varint(&mut stream).await?;
        if !(2..=2_097_152).contains(&len) {
            return Err("Invalid status frame length".into());
        }
        let mut bytes = vec![0; len as usize];
        stream
            .read_exact(&mut bytes)
            .await
            .map_err(|e| e.to_string())?;
        let mut offset = 0;
        if take_varint(&bytes, &mut offset)? != 0 {
            return Err("Unexpected status packet".into());
        }
        let json_len = take_varint(&bytes, &mut offset)?;
        if json_len < 0 || offset + json_len as usize != bytes.len() {
            return Err("Invalid status payload".into());
        }
        let v: Value = serde_json::from_slice(&bytes[offset..]).map_err(|e| e.to_string())?;
        let players = v["players"]["sample"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|p| {
                        Some(Player {
                            name: p["name"].as_str()?.into(),
                            uuid: p["id"].as_str().unwrap_or("").into(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let icon = v["favicon"]
            .as_str()
            .filter(|s| s.starts_with("data:image/png;base64,") && s.len() < 200_000)
            .map(str::to_owned);
        Ok(Status {
            motd: plain_chat(&v["description"]),
            icon,
            version: v["version"]["name"].as_str().unwrap_or("Unknown").into(),
            online: v["players"]["online"].as_u64().unwrap_or(0) as u32,
            max: v["players"]["max"].as_u64().unwrap_or(0) as u32,
            players,
            latency: start.elapsed().as_millis() as u64,
        })
    })
    .await
    .map_err(|_| "Server status timed out".to_string())?
}

async fn rcon_write(s: &mut TcpStream, id: i32, kind: i32, body: &str) -> Result<()> {
    let mut packet = Vec::with_capacity(body.len() + 14);
    packet.extend(((body.len() + 10) as i32).to_le_bytes());
    packet.extend(id.to_le_bytes());
    packet.extend(kind.to_le_bytes());
    packet.extend(body.as_bytes());
    packet.extend([0, 0]);
    s.write_all(&packet).await.map_err(|e| e.to_string())
}
async fn rcon_read(s: &mut TcpStream) -> Result<(i32, i32, String)> {
    let size = s.read_i32_le().await.map_err(|e| e.to_string())?;
    if !(10..=4_194_304).contains(&size) {
        return Err("Invalid RCON packet length".into());
    }
    let mut bytes = vec![0; size as usize];
    s.read_exact(&mut bytes).await.map_err(|e| e.to_string())?;
    if bytes[bytes.len() - 2..] != [0, 0] {
        return Err("Invalid RCON terminator".into());
    }
    Ok((
        i32::from_le_bytes(bytes[0..4].try_into().unwrap()),
        i32::from_le_bytes(bytes[4..8].try_into().unwrap()),
        String::from_utf8_lossy(&bytes[8..bytes.len() - 2]).into(),
    ))
}
pub async fn rcon(host: &str, port: u16, password: &str, command: &str) -> Result<String> {
    timeout(Duration::from_secs(5), async {
        let mut s = TcpStream::connect((host, port))
            .await
            .map_err(|e| e.to_string())?;
        rcon_write(&mut s, 1, 3, password).await?;
        loop {
            let (id, kind, _) = rcon_read(&mut s).await?;
            if id == -1 {
                return Err("RCON authentication failed".into());
            }
            if id == 1 && kind == 2 {
                break;
            }
        }
        rcon_write(&mut s, 2, 2, command).await?;
        let (id, _, mut output) = rcon_read(&mut s).await?;
        if id != 2 {
            return Err("Unexpected RCON response".into());
        }
        // Minecraft fragments responses; gather successive packets without hanging on idle sockets.
        while let Ok(Ok((id, _, body))) =
            timeout(Duration::from_millis(200), rcon_read(&mut s)).await
        {
            if id != 2 {
                break;
            }
            output.push_str(&body);
            if output.len() > 4_194_304 {
                return Err("RCON response too large".into());
            }
        }
        Ok(strip_format(&output))
    })
    .await
    .map_err(|_| "RCON timed out".to_string())?
}

pub fn parse_commands(help: &str) -> Vec<String> {
    let mut commands = vec![];
    for line in help.lines() {
        let line = line.trim().trim_start_matches('/');
        let token = line
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches(':');
        if (help_line_is_command(line) || line.contains(" - "))
            && !token.is_empty()
            && token.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' || c == ':'
            })
        {
            commands.push(token.to_string());
        }
    }
    commands.sort();
    commands.dedup();
    commands
}

fn help_line_is_command(line: &str) -> bool {
    line.contains(':')
        || line.contains(" <")
        || line.contains(" [")
        || line.contains(" (")
        || !line.contains(' ')
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;
    #[test]
    fn formats_chat_and_parses_real_help() {
        assert_eq!(
            plain_chat(&serde_json::json!({"text":"§6Hello", "extra":[{"text":" world"}]})),
            "Hello world"
        );
        assert_eq!(
            parse_commands(
                "--- Help ---\n/list: Players\n/stop: Stops the server\n/list: Duplicate\n/squaremap:map: Plugin command\n/give <targets> <item>"
            ),
            vec!["give", "list", "squaremap:map", "stop"]
        );
    }
    #[tokio::test]
    async fn rcon_authentication_and_fragmented_output() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            assert_eq!(rcon_read(&mut s).await.unwrap(), (1, 3, "password".into()));
            rcon_write(&mut s, 1, 2, "").await.unwrap();
            assert_eq!(rcon_read(&mut s).await.unwrap(), (2, 2, "list".into()));
            rcon_write(&mut s, 2, 0, "There are 1 of ").await.unwrap();
            rcon_write(&mut s, 2, 0, "20 players online: Alex")
                .await
                .unwrap();
        });
        assert_eq!(
            rcon("127.0.0.1", port, "password", "list").await.unwrap(),
            "There are 1 of 20 players online: Alex"
        );
        server.await.unwrap();
    }
    #[tokio::test]
    async fn status_reads_motd_icon_and_players() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (mut s, _) = listener.accept().await.unwrap();
            for _ in 0..2 {
                let n = read_varint(&mut s).await.unwrap();
                let mut bytes = vec![0; n as usize];
                s.read_exact(&mut bytes).await.unwrap();
            }
            let json = serde_json::json!({"version":{"name":"Paper 1.21.4"},"description":{"text":"§6Test world"},"players":{"online":1,"max":20,"sample":[{"name":"Alex","id":"uuid"}]}}).to_string();
            let mut body = vec![0];
            body.extend(varint(json.len() as i32));
            body.extend(json.as_bytes());
            send_frame(&mut s, &body).await.unwrap();
        });
        let result = status("127.0.0.1", port).await.unwrap();
        assert_eq!(result.motd, "Test world");
        assert_eq!(result.online, 1);
        assert_eq!(result.players[0].name, "Alex");
        server.await.unwrap();
    }
}
