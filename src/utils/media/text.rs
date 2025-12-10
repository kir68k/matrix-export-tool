use std::{
    io::Write as StdWrite, path::PathBuf, sync::{Arc, Mutex}
};
use std::fs::OpenOptions;

use crate::utils::media::EventMetadata;

/// Temporary buffer for text messages.
pub struct TextBufferInner {
    pub lines: Vec<String>,
}

#[derive(Clone)]
pub struct TextBuffer {
    inner: Arc<Mutex<TextBufferInner>>,
    pub file: PathBuf,
}

impl TextBufferInner {
    fn new() -> Self {
        Self { lines: Vec::new() }
    }
}

impl TextBuffer {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        let buf = Mutex::new(TextBufferInner::new());
        Self {
            inner: Arc::new(buf),
            file: file.into(),
        }
    }

    pub fn push_line(&self, line: String) {
        let mut lock = self.inner.try_lock();
        if let Ok(ref mut inner) = lock {
            inner.lines.push(line);
        } else {
            tracing::error!("Couldn't get mutex lock for TextBufferInner.");
        }
    }

    pub fn write(&self) -> anyhow::Result<()> {
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(&self.file)?;

        let mut lock = self.inner.lock();
        match lock {
            Ok(ref mut inner) => {
                let output = std::mem::take(&mut inner.lines)
                    .into_iter()
                    .collect::<String>();

                file.write_all(output.as_bytes())?;
                file.flush()?;

                anyhow::Ok(())
            }
            Err(e) => {
                tracing::error!("TextBuffer::write | Mutex lock error: {e}");
                anyhow::bail!("Mutex lock error: {e}");
            }
        }
    }
}

/// Format and store a text event into a buffer.
pub fn process_text_event(body: &str, metadata: &EventMetadata, buffer: &TextBuffer) {
    let formatted = format!("{:?} - {}: {}\n", metadata.timestamp, metadata.sender, body);
    buffer.push_line(formatted);
}
