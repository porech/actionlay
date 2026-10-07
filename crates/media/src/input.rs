//! One seekable source for playback: large reads and a bounded cache retained
//! across seeks. No second source is opened for metadata.
use crate::{MediaError, ffmpeg_info};
use ffmpeg_next as ffmpeg;
use std::{
    collections::VecDeque,
    fs::File,
    io::{self, Read, Seek, SeekFrom},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
const BLOCK_BYTES: usize = 1024 * 1024;
const CACHE_BLOCKS: usize = 32;

struct Block {
    offset: u64,
    data: Vec<u8>,
}
struct CachedReader<R> {
    source: R,
    size: u64,
    position: u64,
    blocks: VecDeque<Block>,
    cancelled: Arc<AtomicBool>,
}
impl<R: Read + Seek> CachedReader<R> {
    fn new(mut source: R, cancelled: Arc<AtomicBool>) -> io::Result<Self> {
        let size = source.seek(SeekFrom::End(0))?;
        Ok(Self {
            source,
            size,
            position: 0,
            blocks: VecDeque::new(),
            cancelled,
        })
    }
}
impl<R: Read + Seek> Read for CachedReader<R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(io::Error::other("read cancelled"));
        }
        if output.is_empty() || self.position >= self.size {
            return Ok(0);
        }
        let offset = self.position / BLOCK_BYTES as u64 * BLOCK_BYTES as u64;
        let block = if let Some(i) = self.blocks.iter().position(|b| b.offset == offset) {
            self.blocks.remove(i).unwrap()
        } else {
            let started = std::time::Instant::now();
            self.source.seek(SeekFrom::Start(offset))?;
            let len = (self.size - offset).min(BLOCK_BYTES as u64) as usize;
            let mut data = vec![0; len];
            let mut got = 0;
            while got < len {
                if self.cancelled.load(Ordering::SeqCst) {
                    return Err(io::Error::other("read cancelled"));
                }
                match self.source.read(&mut data[got..]) {
                    Ok(0) => break,
                    Ok(n) => got += n,
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                }
            }
            if started.elapsed() >= std::time::Duration::from_millis(100) {
                log::debug!(
                    "source read: offset={offset} bytes={got} elapsed_ms={}",
                    started.elapsed().as_millis()
                );
            }
            data.truncate(got);
            Block { offset, data }
        };
        let within = (self.position - offset) as usize;
        let len = output.len().min(block.data.len().saturating_sub(within));
        if len > 0 {
            output[..len].copy_from_slice(&block.data[within..within + len]);
        }
        self.position += len as u64;
        self.blocks.push_back(block);
        if self.blocks.len() > CACHE_BLOCKS {
            self.blocks.pop_front();
        }
        Ok(len)
    }
}
impl<R: Read + Seek> Seek for CachedReader<R> {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let position = match from {
            SeekFrom::Start(p) => i128::from(p),
            SeekFrom::Current(delta) => i128::from(self.position) + i128::from(delta),
            SeekFrom::End(delta) => i128::from(self.size) + i128::from(delta),
        };
        self.position = u64::try_from(position)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid seek"))?;
        Ok(self.position)
    }
}

pub(crate) fn open(
    path: &Path,
    cancelled: Arc<AtomicBool>,
) -> Result<ffmpeg::format::context::Input, MediaError> {
    ffmpeg_info::init();
    let source = File::open(path).map_err(|e| MediaError::Io(e.to_string()))?;
    open_source(source, path.to_str(), cancelled)
}

pub(crate) fn open_source<R: Read + Seek + Send + 'static>(
    source: R,
    filename: Option<&str>,
    cancelled: Arc<AtomicBool>,
) -> Result<ffmpeg::format::context::Input, MediaError> {
    let source =
        CachedReader::new(source, cancelled.clone()).map_err(|e| MediaError::Io(e.to_string()))?;
    let io = ffmpeg::format::context::StreamIo::from_read_seek_with_capacity(source, BLOCK_BYTES)?;
    Ok(ffmpeg::format::input_from_stream_with_interrupt(
        io,
        filename,
        None,
        move || cancelled.load(Ordering::SeqCst),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    struct Counting {
        input: Cursor<Vec<u8>>,
        reads: Vec<usize>,
    }
    impl Read for Counting {
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.reads.push(b.len());
            self.input.read(b)
        }
    }
    impl Seek for Counting {
        fn seek(&mut self, s: SeekFrom) -> io::Result<u64> {
            self.input.seek(s)
        }
    }
    #[test]
    fn small_reads_and_seeks_use_large_cached_blocks() {
        let source = Counting {
            input: Cursor::new(vec![42; 3 * BLOCK_BYTES]),
            reads: vec![],
        };
        let mut reader = CachedReader::new(source, Arc::new(AtomicBool::new(false))).unwrap();
        let mut b = [0; 16];
        for _ in 0..100 {
            reader.read_exact(&mut b).unwrap();
        }
        reader
            .seek(SeekFrom::Start(BLOCK_BYTES as u64 + 40))
            .unwrap();
        reader.read_exact(&mut b).unwrap();
        reader.seek(SeekFrom::Start(0)).unwrap();
        reader.read_exact(&mut b).unwrap();
        assert_eq!(b, [42; 16]);
        assert_eq!(reader.source.reads, vec![BLOCK_BYTES, BLOCK_BYTES]);
    }
    #[test]
    fn cache_eviction_eof_and_cancellation() {
        let cancel = Arc::new(AtomicBool::new(false));
        let mut reader = CachedReader::new(
            Cursor::new(vec![7; (CACHE_BLOCKS + 1) * BLOCK_BYTES + 3]),
            cancel.clone(),
        )
        .unwrap();
        let mut b = [0; 8];
        for i in 0..=CACHE_BLOCKS {
            reader
                .seek(SeekFrom::Start((i * BLOCK_BYTES) as u64))
                .unwrap();
            reader.read_exact(&mut b).unwrap();
        }
        assert_eq!(reader.blocks.len(), CACHE_BLOCKS);
        assert!(!reader.blocks.iter().any(|b| b.offset == 0));
        reader.seek(SeekFrom::End(-3)).unwrap();
        assert_eq!(reader.read(&mut b).unwrap(), 3);
        assert_eq!(reader.read(&mut b).unwrap(), 0);
        cancel.store(true, Ordering::SeqCst);
        assert!(reader.read(&mut b).is_err());
        assert!(reader.seek(SeekFrom::Start(0)).is_ok());
        assert!(reader.seek(SeekFrom::Current(-1)).is_err());
    }
}
