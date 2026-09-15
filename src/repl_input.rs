//! 终端唯一读取器。独立线程不占 Tokio blocking pool，退出不会等待 stdin。
use crate::error::AppError;
use std::io::{BufRead, Write};
use tokio::sync::mpsc;

pub(crate) type Input = mpsc::Receiver<Result<String, AppError>>;

fn read_message(reader: &mut impl BufRead) -> Result<Option<String>, AppError> {
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok((!lines.is_empty()).then(|| lines.join("\n")));
        }
        let line = line.trim_end_matches(['\n', '\r']);
        if let Some(part) = line.strip_suffix('\\') {
            lines.push(part.to_string());
        } else {
            lines.push(line.to_string());
            return Ok(Some(lines.join("\n")));
        }
    }
}

pub(crate) fn stdin_input() -> Input {
    let (sender, receiver) = mpsc::channel(1);
    std::thread::spawn(move || {
        let stdin = std::io::stdin();
        let mut reader = stdin.lock();
        loop {
            match read_message(&mut reader) {
                Ok(Some(line)) => {
                    if sender.blocking_send(Ok(line)).is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(e) => {
                    let _ = sender.blocking_send(Err(e));
                    break;
                }
            }
        }
    });
    receiver
}

pub(crate) fn prompt(text: &str) {
    print!("{text}");
    let _ = std::io::stdout().flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multiline_then_next_message_and_eof() {
        let mut r = std::io::Cursor::new("第一行\\\r\n第二行\n命名\n");
        assert_eq!(
            read_message(&mut r).unwrap().as_deref(),
            Some("第一行\n第二行")
        );
        assert_eq!(read_message(&mut r).unwrap().as_deref(), Some("命名"));
        assert_eq!(read_message(&mut r).unwrap(), None);
    }
    #[test]
    fn eof_submits_partial_multiline() {
        let mut r = std::io::Cursor::new("内容\\\n");
        assert_eq!(read_message(&mut r).unwrap().as_deref(), Some("内容"));
        assert_eq!(read_message(&mut r).unwrap(), None);
    }
}
