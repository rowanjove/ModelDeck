#[derive(Debug, Default)]
pub struct SseParser {
    buffer: String,
}

impl SseParser {
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        self.buffer
            .push_str(&chunk.replace("\r\n", "\n").replace('\r', "\n"));
        let mut events = Vec::new();
        while let Some(index) = self.buffer.find("\n\n") {
            let block = self.buffer[..index].to_string();
            self.buffer.replace_range(..index + 2, "");
            if let Some(data) = event_data(&block) {
                events.push(data);
            }
        }
        events
    }
}

fn event_data(block: &str) -> Option<String> {
    let mut data = Vec::new();
    for line in block.lines() {
        let Some(rest) = line.strip_prefix("data:") else {
            continue;
        };
        data.push(rest.strip_prefix(' ').unwrap_or(rest));
    }
    if data.is_empty() {
        None
    } else {
        Some(data.join("\n"))
    }
}

#[derive(Debug, Default)]
pub struct LineParser {
    buffer: String,
}

impl LineParser {
    pub fn push(&mut self, chunk: &str) -> Vec<String> {
        self.buffer
            .push_str(&chunk.replace("\r\n", "\n").replace('\r', "\n"));
        let mut lines = Vec::new();
        while let Some(index) = self.buffer.find('\n') {
            let line = self.buffer[..index].trim().to_string();
            self.buffer.replace_range(..index + 1, "");
            if !line.is_empty() {
                lines.push(line);
            }
        }
        lines
    }

    pub fn finish(&mut self) -> Option<String> {
        let line = self.buffer.trim().to_string();
        self.buffer.clear();
        if line.is_empty() {
            None
        } else {
            Some(line)
        }
    }
}

#[derive(Debug, Default)]
pub struct Utf8ChunkDecoder {
    buffer: Vec<u8>,
}

impl Utf8ChunkDecoder {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn decode(&mut self, chunk: &[u8]) -> String {
        self.buffer.extend_from_slice(chunk);
        match std::str::from_utf8(&self.buffer) {
            Ok(_) => unsafe { String::from_utf8_unchecked(std::mem::take(&mut self.buffer)) },
            Err(err) => {
                let valid_up_to = err.valid_up_to();
                if valid_up_to > 0 {
                    let valid_bytes = self.buffer.drain(..valid_up_to).collect::<Vec<u8>>();
                    unsafe { String::from_utf8_unchecked(valid_bytes) }
                } else if err.error_len().is_some() {
                    // 确认为非法序列，消耗 1 字节避免死循环
                    self.buffer.remove(0);
                    "\u{FFFD}".to_string()
                } else {
                    // 仅为未完成的 UTF-8 序列尾部，保留在 buffer 等待下一个分片
                    String::new()
                }
            }
        }
    }

    pub fn finish(&mut self) -> String {
        if self.buffer.is_empty() {
            String::new()
        } else {
            String::from_utf8_lossy(&std::mem::take(&mut self.buffer)).into_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sse_split_across_chunks() {
        let mut parser = SseParser::default();
        assert!(parser.push("data: {\"a").is_empty());
        let events = parser.push("\":1}\n\ndata: [DONE]\n\n");
        assert_eq!(events, vec!["{\"a\":1}".to_string(), "[DONE]".to_string()]);
    }

    #[test]
    fn parses_ndjson_lines() {
        let mut parser = LineParser::default();
        let first = parser.push("{\"message\":{\"content\":\"He\"}}\n{\"mess");
        assert_eq!(first.len(), 1);
        let rest = parser.push("age\":{\"content\":\"llo\"}}\n");
        assert_eq!(rest.len(), 1);
        assert!(parser.finish().is_none());
    }

    #[test]
    fn utf8_chunk_decoder_handles_split_multibyte_chinese_and_emoji() {
        let mut decoder = Utf8ChunkDecoder::new();
        let text = "你好，世界！🌍🚀";
        let bytes = text.as_bytes();

        // 故意以 2 字节为一步打碎分包（中文每个汉字3字节，必然在中间被切断）
        let mut decoded = String::new();
        for chunk in bytes.chunks(2) {
            decoded.push_str(&decoder.decode(chunk));
        }
        decoded.push_str(&decoder.finish());

        assert_eq!(decoded, text);
    }
}
