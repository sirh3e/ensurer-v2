/// Line-buffered SSE parser over a `hyper::body::Incoming` stream.
///
/// Buffers bytes until a blank line is seen (`\n\n`), then emits one `SseEvent`
/// per event block. Fields `id:` and `data:` are extracted; others are ignored.
use bytes::BytesMut;
use http_body_util::BodyExt;
use hyper::body::Incoming;

#[derive(Debug, Default)]
pub struct SseEvent {
    pub id: Option<String>,
    pub data: Option<String>,
}

pub struct SseStream {
    body: Incoming,
    buf: BytesMut,
}

impl SseStream {
    pub fn new(body: Incoming) -> Self {
        Self { body, buf: BytesMut::new() }
    }

    /// Returns the next complete SSE event, or `None` when the stream ends.
    pub async fn next(&mut self) -> Option<Result<SseEvent, String>> {
        loop {
            // Check if we already have a complete event in the buffer.
            if let Some(pos) = find_double_newline(&self.buf) {
                let block = self.buf.split_to(pos + 2); // consume through `\n\n`
                let event = parse_event(&block);
                return Some(Ok(event));
            }

            // Need more data.
            match self.body.frame().await {
                Some(Ok(frame)) => {
                    if let Ok(data) = frame.into_data() {
                        self.buf.extend_from_slice(&data);
                    }
                    // metadata frames (trailers) are silently skipped
                }
                Some(Err(e)) => return Some(Err(e.to_string())),
                None => {
                    // Stream ended — flush a partial event if anything is buffered.
                    if !self.buf.is_empty() {
                        let block = self.buf.split();
                        return Some(Ok(parse_event(&block)));
                    }
                    return None;
                }
            }
        }
    }
}

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

fn parse_event(block: &[u8]) -> SseEvent {
    let text = std::str::from_utf8(block).unwrap_or("");
    let mut event = SseEvent::default();
    for line in text.lines() {
        if let Some(v) = line.strip_prefix("id:") {
            event.id = Some(v.trim_start().to_string());
        } else if let Some(v) = line.strip_prefix("data:") {
            event.data = Some(v.trim_start().to_string());
        }
    }
    event
}
