//! Piece-table text buffer for super-large files (goal.md #7).
//!
//! The file is streamed in as read-only chunks; edits go into a separate
//! append-only "add" buffer. The document is a list of pieces, each a byte
//! range into one of those buffers. Newline offsets are cached per buffer so
//! line lookups are a binary search, not a scan.

use std::ops::Range;

/// Byte range into one buffer. `buf == 0` is the add buffer.
#[derive(Debug, Clone, Copy)]
struct Piece {
    buf: usize,
    start: usize,
    len: usize,
}

pub struct PieceTable {
    /// `buffers[0]` is the append-only edit buffer; the rest are the
    /// original file, one entry per streamed chunk.
    buffers: Vec<String>,
    /// Sorted byte offsets of every '\n' in each buffer.
    newlines: Vec<Vec<usize>>,
    pieces: Vec<Piece>,
}

fn newline_offsets(s: &str) -> Vec<usize> {
    s.bytes()
        .enumerate()
        .filter(|(_, b)| *b == b'\n')
        .map(|(i, _)| i)
        .collect()
}

impl PieceTable {
    pub fn new() -> Self {
        Self {
            buffers: vec![String::new()],
            newlines: vec![Vec::new()],
            pieces: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn from_text(text: &str) -> Self {
        let mut t = Self::new();
        t.push_chunk(text.to_owned());
        t
    }

    /// Append a streamed chunk of the original file at the end of the
    /// document. Correct even if the user already edited earlier content:
    /// not-yet-loaded bytes always belong at the document's end.
    pub fn push_chunk(&mut self, chunk: String) {
        if chunk.is_empty() {
            return;
        }
        self.newlines.push(newline_offsets(&chunk));
        self.pieces.push(Piece {
            buf: self.buffers.len(),
            start: 0,
            len: chunk.len(),
        });
        self.buffers.push(chunk);
    }

    pub fn len_bytes(&self) -> usize {
        self.pieces.iter().map(|p| p.len).sum()
    }

    fn piece_newlines(&self, p: &Piece) -> usize {
        let nl = &self.newlines[p.buf];
        nl.partition_point(|&o| o < p.start + p.len) - nl.partition_point(|&o| o < p.start)
    }

    pub fn newline_count(&self) -> usize {
        self.pieces.iter().map(|p| self.piece_newlines(p)).sum()
    }

    /// Number of lines (a trailing newline still counts one final empty line).
    pub fn line_count(&self) -> usize {
        self.newline_count() + 1
    }

    /// Global byte offset where line `i` (0-based) starts.
    pub fn line_start(&self, i: usize) -> usize {
        if i == 0 {
            return 0;
        }
        // Find the (i-1)-th newline; the line starts right after it.
        let k = i - 1;
        let mut seen = 0usize;
        let mut global = 0usize;
        for p in &self.pieces {
            let n = self.piece_newlines(p);
            if seen + n > k {
                let nl = &self.newlines[p.buf];
                let base = nl.partition_point(|&o| o < p.start);
                let pos_in_buf = nl[base + (k - seen)];
                return global + (pos_in_buf - p.start) + 1;
            }
            seen += n;
            global += p.len;
        }
        self.len_bytes()
    }

    /// The text of line `i`, without its trailing newline.
    pub fn line(&self, i: usize) -> String {
        let start = self.line_start(i);
        let mut out = String::new();
        let mut pos = 0usize;
        for p in &self.pieces {
            if pos + p.len <= start {
                pos += p.len;
                continue;
            }
            let from = start.max(pos) - pos + p.start;
            let slice = &self.buffers[p.buf][from..p.start + p.len];
            match slice.find('\n') {
                Some(nl) => {
                    out.push_str(&slice[..nl]);
                    return out;
                }
                None => out.push_str(slice),
            }
            pos += p.len;
        }
        out
    }

    /// Split any piece containing `offset` so a piece boundary lands exactly
    /// there; returns the index of the piece that starts at `offset` (or
    /// `pieces.len()` when `offset` is the end of the document).
    fn split_at(&mut self, offset: usize) -> usize {
        let mut pos = 0usize;
        for i in 0..self.pieces.len() {
            let p = self.pieces[i];
            if offset < pos + p.len {
                if offset == pos {
                    return i;
                }
                let cut = offset - pos;
                self.pieces[i].len = cut;
                self.pieces.insert(
                    i + 1,
                    Piece {
                        buf: p.buf,
                        start: p.start + cut,
                        len: p.len - cut,
                    },
                );
                return i + 1;
            }
            pos += p.len;
        }
        self.pieces.len()
    }

    /// Insert `text` at the global byte `offset` (must be a char boundary).
    pub fn insert(&mut self, offset: usize, text: &str) {
        if text.is_empty() {
            return;
        }
        let add_start = self.buffers[0].len();
        self.newlines[0].extend(newline_offsets(text).into_iter().map(|o| o + add_start));
        self.buffers[0].push_str(text);

        let i = self.split_at(offset);
        // Typing appends run after run to the add buffer; extend the previous
        // piece instead of growing the piece list one char at a time.
        if i > 0 {
            let prev = &mut self.pieces[i - 1];
            if prev.buf == 0 && prev.start + prev.len == add_start {
                prev.len += text.len();
                return;
            }
        }
        self.pieces.insert(
            i,
            Piece {
                buf: 0,
                start: add_start,
                len: text.len(),
            },
        );
    }

    /// Delete the global byte range (bounds must be char boundaries).
    pub fn delete(&mut self, range: Range<usize>) {
        if range.is_empty() {
            return;
        }
        let first = self.split_at(range.start);
        let last = self.split_at(range.end);
        self.pieces.drain(first..last);
    }

    /// The whole document as text (walks every piece — test helper).
    #[cfg(test)]
    pub fn text(&self) -> String {
        let mut out = String::with_capacity(self.len_bytes());
        for p in &self.pieces {
            out.push_str(&self.buffers[p.buf][p.start..p.start + p.len]);
        }
        out
    }

    pub fn write_to<W: std::io::Write>(&self, w: &mut W) -> std::io::Result<()> {
        for p in &self.pieces {
            w.write_all(self.buffers[p.buf][p.start..p.start + p.len].as_bytes())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_across_chunks() {
        let mut t = PieceTable::new();
        t.push_chunk("hello\nwor".into());
        t.push_chunk("ld\nlast".into());
        assert_eq!(t.line_count(), 3);
        assert_eq!(t.line(0), "hello");
        assert_eq!(t.line(1), "world");
        assert_eq!(t.line(2), "last");
        assert_eq!(t.line_start(2), 12);
    }

    #[test]
    fn insert_middle_and_typing_coalesces() {
        let mut t = PieceTable::from_text("ab\ncd");
        t.insert(1, "X");
        t.insert(2, "Y"); // right after the X piece -> should extend it
        assert_eq!(t.text(), "aXYb\ncd");
        assert_eq!(t.pieces.len(), 3);
        assert_eq!(t.line(0), "aXYb");
        assert_eq!(t.line(1), "cd");
    }

    #[test]
    fn insert_newline_updates_lines() {
        let mut t = PieceTable::from_text("abcd");
        t.insert(2, "\n");
        assert_eq!(t.line_count(), 2);
        assert_eq!(t.line(0), "ab");
        assert_eq!(t.line(1), "cd");
    }

    #[test]
    fn delete_range_and_join_lines() {
        let mut t = PieceTable::from_text("one\ntwo\nthree");
        t.delete(3..4); // the first newline
        assert_eq!(t.text(), "onetwo\nthree");
        assert_eq!(t.line_count(), 2);
        t.delete(0..6);
        assert_eq!(t.text(), "\nthree");
    }

    #[test]
    fn edits_at_ends() {
        let mut t = PieceTable::from_text("mid");
        t.insert(0, ">");
        t.insert(t.len_bytes(), "<");
        assert_eq!(t.text(), ">mid<");
        t.delete(0..1);
        t.delete(t.len_bytes() - 1..t.len_bytes());
        assert_eq!(t.text(), "mid");
    }

    #[test]
    fn empty_and_trailing_newline() {
        let t = PieceTable::new();
        assert_eq!(t.line_count(), 1);
        assert_eq!(t.line(0), "");
        let t = PieceTable::from_text("a\n");
        assert_eq!(t.line_count(), 2);
        assert_eq!(t.line(1), "");
    }

    #[test]
    fn streamed_chunk_lands_at_end_after_edit() {
        let mut t = PieceTable::new();
        t.push_chunk("first\n".into());
        t.insert(0, "// header\n");
        t.push_chunk("second\n".into());
        assert_eq!(t.text(), "// header\nfirst\nsecond\n");
    }
}
