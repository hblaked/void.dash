//! Follow the current Minecraft log across appends, truncation, and rotation.
use std::{path::Path, time::SystemTime};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

#[derive(Default)]
pub struct LogTail {
    offset: u64,
    prefix: Vec<u8>,
    pending: Vec<u8>,
    created: Option<SystemTime>,
    initialized: bool,
}
impl LogTail {
    pub async fn read(&mut self, path: &Path) -> std::io::Result<Vec<String>> {
        let mut file = tokio::fs::File::open(path).await?;
        let metadata = file.metadata().await?;
        let mut head = vec![0; metadata.len().min(256) as usize];
        file.read_exact(&mut head).await?;
        let created = metadata.created().ok();
        let replaced = self.initialized
            && (metadata.len() < self.offset
                || created != self.created
                || !head.starts_with(&self.prefix));
        let initial = !self.initialized;
        if initial || replaced {
            self.offset = if initial {
                metadata.len().saturating_sub(1_048_576)
            } else {
                0
            };
            self.pending.clear();
            self.prefix = head.clone();
            self.created = created;
            self.initialized = true;
        } else if self.prefix.is_empty() {
            self.prefix = head;
        }
        file.seek(std::io::SeekFrom::Start(self.offset)).await?;
        let mut bytes = Vec::new();
        file.take(1_048_576).read_to_end(&mut bytes).await?;
        let skipped_prefix = initial && self.offset > 0;
        self.offset += bytes.len() as u64;
        self.pending.extend(bytes);
        let mut lines = vec![];
        if let Some(last) = self.pending.iter().rposition(|b| *b == b'\n') {
            let complete: Vec<_> = self.pending.drain(..=last).collect();
            for (index, line) in complete.split(|b| *b == b'\n').enumerate() {
                if line.is_empty() || skipped_prefix && index == 0 {
                    continue;
                }
                lines.push(
                    String::from_utf8_lossy(line)
                        .trim_end_matches('\r')
                        .to_string(),
                );
            }
        }
        // Bound pathological unterminated output without dropping ordinary log lines.
        if self.pending.len() > 1_048_576 {
            lines.push(String::from_utf8_lossy(&std::mem::take(&mut self.pending)).into());
        }
        Ok(lines)
    }
}

pub fn clean_message(line: &str) -> String {
    let mut clean = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        } else if c == '§' {
            chars.next();
        } else if !c.is_control() || c == '\t' {
            clean.push(c);
        }
    }
    clean
}
pub fn message_body(line: &str) -> &str {
    let mut body = line.trim();
    while body.starts_with('[') {
        if let Some((_, rest)) = body.split_once("] ").or_else(|| body.split_once("]: ")) {
            body = rest.trim_start();
        } else {
            break;
        }
    }
    body
}
pub fn list_report(line: &str) -> Option<(u32, Vec<String>)> {
    let body = message_body(line);
    let rest = body.strip_prefix("There are ")?;
    let count = rest.split_whitespace().next()?.parse::<u32>().ok()?;
    let (_, names) = rest.split_once("players online:")?;
    let names = if count == 0 {
        vec![]
    } else {
        names
            .trim()
            .split(", ")
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
            .collect()
    };
    Some((count, names))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn follows_partial_lines_truncation_and_rotation_without_repeating_output() {
        let directory = std::env::temp_dir().join(format!("void-tail-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir_all(&directory).await.unwrap();
        let path = directory.join("latest.log");
        tokio::fs::write(&path, "first\npartial").await.unwrap();
        let mut tail = LogTail::default();
        assert_eq!(tail.read(&path).await.unwrap(), vec!["first"]);
        assert!(tail.read(&path).await.unwrap().is_empty());
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .await
            .unwrap();
        file.write_all(b" completed\nplayer left\n").await.unwrap();
        drop(file);
        assert_eq!(
            tail.read(&path).await.unwrap(),
            vec!["partial completed", "player left"]
        );
        tokio::fs::rename(&path, directory.join("previous.log"))
            .await
            .unwrap();
        tokio::fs::write(&path, "new log\n").await.unwrap();
        assert_eq!(tail.read(&path).await.unwrap(), vec!["new log"]);
        tokio::fs::write(&path, "x\n").await.unwrap();
        assert_eq!(tail.read(&path).await.unwrap(), vec!["x"]);
        tokio::fs::remove_dir_all(directory).await.unwrap();
    }
    #[test]
    fn parses_zero_list_and_keeps_chat_out_of_player_metadata() {
        assert_eq!(
            list_report("[12:00:01 INFO]: There are 0 of a max of 20 players online:&#x20;"),
            Some((0, vec![]))
        );
        assert_eq!(
            list_report("[Server thread/INFO]: There are 1 of a max of 20 players online: Alex"),
            Some((1, vec!["Alex".into()]))
        );
        assert!(list_report("<Alex> There are 0 of a max of 20 players online:").is_none());
        assert_eq!(
            clean_message("\u{1b}[32mAlex joined\u{1b}[0m"),
            "Alex joined"
        );
    }
}
