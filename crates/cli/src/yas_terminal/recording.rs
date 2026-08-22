//! `YASREC1` native TerminalFrame recording codec.

use std::io::Write;

use yas_wire::{Encode, terminal};

#[cfg(test)]
use yas_wire::Decode;

const MAGIC: &[u8; 8] = b"YASREC1\n";
const HEADER_BYTES: u32 = 36;
const TICKS_PER_SECOND: u64 = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Header {
    pub grid_codec: u16,
    pub terminal_handle: u64,
    pub generation: u32,
    pub rows: u16,
    pub cols: u16,
    pub view_id: u32,
    pub first_sequence: u32,
}

impl Header {
    fn validate(&self) -> Result<(), String> {
        if self.grid_codec != 1 {
            return Err(format!(
                "YASREC1 only supports Terminal grid codec 1, got {}",
                self.grid_codec
            ));
        }
        if self.terminal_handle == 0
            || self.generation == 0
            || self.rows == 0
            || self.cols == 0
            || self.view_id == 0
        {
            return Err("YASREC1 header contains a zero required field".into());
        }
        Ok(())
    }

    fn encode(&self, output: &mut impl Write) -> Result<(), String> {
        self.validate()?;
        output.write_all(MAGIC).map_err(io_error)?;
        output
            .write_all(&HEADER_BYTES.to_le_bytes())
            .and_then(|_| output.write_all(&0u16.to_le_bytes()))
            .and_then(|_| output.write_all(&self.grid_codec.to_le_bytes()))
            .and_then(|_| output.write_all(&self.terminal_handle.to_le_bytes()))
            .and_then(|_| output.write_all(&self.generation.to_le_bytes()))
            .and_then(|_| output.write_all(&self.rows.to_le_bytes()))
            .and_then(|_| output.write_all(&self.cols.to_le_bytes()))
            .and_then(|_| output.write_all(&self.view_id.to_le_bytes()))
            .and_then(|_| output.write_all(&self.first_sequence.to_le_bytes()))
            .and_then(|_| output.write_all(&TICKS_PER_SECOND.to_le_bytes()))
            .map_err(io_error)
    }
}

pub(super) struct Writer<W> {
    output: W,
    header: Header,
    next_sequence: u32,
    last_timestamp: Option<u64>,
    frame_count: u32,
}

impl<W: Write> Writer<W> {
    pub(super) fn new(mut output: W, header: Header) -> Result<Self, String> {
        header.encode(&mut output)?;
        Ok(Self {
            next_sequence: header.first_sequence,
            output,
            header,
            last_timestamp: None,
            frame_count: 0,
        })
    }

    pub(super) fn write_frame(
        &mut self,
        timestamp_ticks: u64,
        frame: &terminal::TerminalFrame,
    ) -> Result<(), String> {
        validate_frame(
            &self.header,
            self.next_sequence,
            self.frame_count == 0,
            frame,
        )?;
        if self
            .last_timestamp
            .is_some_and(|previous| timestamp_ticks < previous)
        {
            return Err("YASREC1 frame timestamps went backwards".into());
        }
        let payload = frame
            .encode()
            .map_err(|error| format!("encode YASREC1 TerminalFrame: {error}"))?;
        let length = u32::try_from(payload.len())
            .map_err(|_| "YASREC1 TerminalFrame exceeds u32 length".to_string())?;
        self.output
            .write_all(&timestamp_ticks.to_le_bytes())
            .and_then(|_| self.output.write_all(&length.to_le_bytes()))
            .and_then(|_| self.output.write_all(&payload))
            .map_err(io_error)?;
        self.next_sequence = self.next_sequence.wrapping_add(1);
        self.last_timestamp = Some(timestamp_ticks);
        self.frame_count = self.frame_count.saturating_add(1);
        Ok(())
    }

    pub(super) fn finish(mut self) -> Result<W, String> {
        self.output.flush().map_err(io_error)?;
        Ok(self.output)
    }
}

fn validate_frame(
    header: &Header,
    expected_sequence: u32,
    first: bool,
    frame: &terminal::TerminalFrame,
) -> Result<(), String> {
    if frame.view_id != header.view_id {
        return Err("YASREC1 TerminalFrame belongs to a different view".into());
    }
    if frame.frame_sequence != expected_sequence {
        return Err(format!(
            "YASREC1 TerminalFrame sequence {}, expected {expected_sequence}",
            frame.frame_sequence
        ));
    }
    if first && frame.frame_flags & yas_wire::schema::terminal::FRAME_KEYFRAME as u16 == 0 {
        return Err("YASREC1 recording must begin with a Terminal keyframe".into());
    }
    Ok(())
}

fn io_error(error: std::io::Error) -> String {
    format!("write YASREC1 recording: {error}")
}

#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
struct Recording {
    header: Header,
    frames: Vec<(u64, terminal::TerminalFrame)>,
}

#[cfg(test)]
fn decode(bytes: &[u8]) -> Result<Recording, String> {
    if !bytes.starts_with(MAGIC) {
        return Err("invalid YASREC1 magic".into());
    }
    let mut input = Cursor::new(&bytes[MAGIC.len()..]);
    let header_bytes = input.u32()?;
    if header_bytes != HEADER_BYTES {
        return Err(format!("unsupported YASREC1 header length {header_bytes}"));
    }
    let flags = input.u16()?;
    if flags != 0 {
        return Err("unsupported YASREC1 header flags".into());
    }
    let header = Header {
        grid_codec: input.u16()?,
        terminal_handle: input.u64()?,
        generation: input.u32()?,
        rows: input.u16()?,
        cols: input.u16()?,
        view_id: input.u32()?,
        first_sequence: input.u32()?,
    };
    if input.u64()? != TICKS_PER_SECOND {
        return Err("unsupported YASREC1 timestamp timebase".into());
    }
    header.validate()?;

    let mut frames = Vec::new();
    let mut next_sequence = header.first_sequence;
    let mut last_timestamp = None;
    while !input.is_empty() {
        let timestamp = input.u64()?;
        if last_timestamp.is_some_and(|previous| timestamp < previous) {
            return Err("YASREC1 frame timestamps went backwards".into());
        }
        let length = input.u32()? as usize;
        if length == 0 {
            return Err("YASREC1 contains an empty TerminalFrame".into());
        }
        let frame = terminal::TerminalFrame::decode(input.take(length)?)
            .map_err(|error| format!("decode YASREC1 TerminalFrame: {error}"))?;
        validate_frame(&header, next_sequence, frames.is_empty(), &frame)?;
        next_sequence = next_sequence.wrapping_add(1);
        last_timestamp = Some(timestamp);
        frames.push((timestamp, frame));
    }
    Ok(Recording { header, frames })
}

#[cfg(test)]
struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

#[cfg(test)]
impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], String> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or_else(|| "YASREC1 length overflow".to_string())?;
        let bytes = self
            .bytes
            .get(self.offset..end)
            .ok_or_else(|| "truncated YASREC1 recording".to_string())?;
        self.offset = end;
        Ok(bytes)
    }

    fn u16(&mut self) -> Result<u16, String> {
        Ok(u16::from_le_bytes(
            self.take(2)?.try_into().expect("two bytes"),
        ))
    }

    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn u64(&mut self) -> Result<u64, String> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> Header {
        Header {
            grid_codec: 1,
            terminal_handle: 7,
            generation: 2,
            rows: 1,
            cols: 1,
            view_id: 9,
            first_sequence: 4,
        }
    }

    fn frame() -> terminal::TerminalFrame {
        let flags = yas_wire::schema::terminal::FRAME_KEYFRAME as u16
            | yas_wire::schema::terminal::FRAME_DIMENSIONS as u16
            | yas_wire::schema::terminal::FRAME_CURSOR as u16
            | yas_wire::schema::terminal::FRAME_MODES as u16
            | yas_wire::schema::terminal::FRAME_SCROLLBACK as u16
            | yas_wire::schema::terminal::FRAME_VIEW_OFFSET as u16
            | yas_wire::schema::terminal::FRAME_TITLE as u16;
        terminal::TerminalFrame {
            view_id: 9,
            frame_sequence: 4,
            frame_flags: flags,
            base_sequence: None,
            grid_payload: terminal::Grid {
                dimensions: Some((1, 1)),
                cursor: Some((0, 0)),
                modes: Some(0),
                scrollback_lines: Some(0),
                scroll_offset: Some(0),
                title: Some("recording".into()),
                operations: vec![terminal::GridOperation::PatchRun {
                    start_cell: 0,
                    cells: vec![[0; 12]],
                }],
                components: Vec::new(),
            }
            .encode_codec1(flags, 4096, None)
            .unwrap(),
        }
    }

    #[test]
    fn native_recording_round_trips_metadata_timing_and_frame() {
        let mut writer = Writer::new(Vec::new(), header()).unwrap();
        writer.write_frame(1234, &frame()).unwrap();
        let bytes = writer.finish().unwrap();
        let recording = decode(&bytes).unwrap();
        assert_eq!(recording.header, header());
        assert_eq!(recording.frames, vec![(1234, frame())]);
    }

    #[test]
    fn invalid_magic_is_rejected_explicitly() {
        assert!(decode(b"NOTYAS!\n").unwrap_err().contains("magic"));
    }

    #[test]
    fn writer_rejects_non_keyframe_first_frame() {
        let mut delta = frame();
        delta.frame_flags &= !(yas_wire::schema::terminal::FRAME_KEYFRAME as u16
            | yas_wire::schema::terminal::FRAME_DIMENSIONS as u16);
        delta.grid_payload = terminal::Grid {
            dimensions: None,
            cursor: Some((0, 0)),
            modes: Some(0),
            scrollback_lines: Some(0),
            scroll_offset: Some(0),
            title: Some(String::new()),
            operations: Vec::new(),
            components: Vec::new(),
        }
        .encode_codec1(delta.frame_flags, 4096, Some((1, 1)))
        .unwrap();
        let mut writer = Writer::new(Vec::new(), header()).unwrap();
        assert!(
            writer
                .write_frame(0, &delta)
                .unwrap_err()
                .contains("keyframe")
        );
    }
}
