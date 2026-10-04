//! tmux control mode carries the original PTY bytes, rather than screen redraws.
//! The browser owns rendering and scrollback; tmux only retains the remote shell.
use std::{collections::VecDeque, fmt::Write};

const MAX_LINE: usize = 256 * 1024;
const MAX_SNAPSHOT: usize = 4 * 1024 * 1024;
pub const HISTORY: usize = 5000;
const BLOCKS: usize = 6;

#[derive(Default)]
struct Snapshot {
    lines: VecDeque<Vec<u8>>,
    bytes: usize,
}
impl Snapshot {
    fn push(&mut self, line: &[u8]) -> Result<(), &'static str> {
        let line = decode(line, true)?;
        self.bytes += line.len() + 2;
        self.lines.push_back(line);
        while self.bytes > MAX_SNAPSHOT || self.lines.len() > HISTORY + 120 {
            let old = self.lines.pop_front().ok_or("snapshot limit")?;
            self.bytes -= old.len() + 2;
        }
        Ok(())
    }
    fn paint(&self, out: &mut Vec<u8>) {
        for (i, line) in self.lines.iter().enumerate() {
            if i != 0 {
                out.extend_from_slice(b"\r\n");
            }
            out.extend_from_slice(line);
        }
    }
}

pub struct Stream {
    pending: Vec<u8>,
    guard: Option<Vec<u8>>,
    step: usize,
    active: Snapshot,
    saved: Snapshot,
    incomplete: Vec<u8>,
    state: Vec<u8>,
    pane: Vec<u8>,
    cols: u32,
    rows: u32,
}
impl Stream {
    pub fn new(cols: u32, rows: u32) -> Self {
        Self {
            pending: Vec::new(),
            guard: None,
            step: 0,
            active: Snapshot::default(),
            saved: Snapshot::default(),
            incomplete: Vec::new(),
            state: Vec::new(),
            pane: Vec::new(),
            cols,
            rows,
        }
    }
    pub fn ready(&self) -> bool {
        self.step >= BLOCKS
    }
    pub fn feed(&mut self, bytes: &[u8]) -> Result<Vec<u8>, &'static str> {
        let mut out = Vec::new();
        for part in bytes.split_inclusive(|b| *b == b'\n') {
            if self.pending.len() + part.len() > MAX_LINE {
                return Err("tmux record limit");
            }
            self.pending.extend_from_slice(part);
            if part.last() == Some(&b'\n') {
                let mut line = std::mem::take(&mut self.pending);
                line.pop();
                self.line(&line, &mut out)?;
            }
        }
        Ok(out)
    }
    fn line(&mut self, line: &[u8], out: &mut Vec<u8>) -> Result<(), &'static str> {
        if let Some(guard) = &self.guard {
            if line.strip_prefix(b"%error ") == Some(guard.as_slice()) {
                return Err("tmux command failed");
            }
            if line.strip_prefix(b"%end ") == Some(guard.as_slice()) {
                self.guard = None;
                if !self.ready() {
                    self.step += 1;
                    if self.ready() {
                        self.restore(out)?;
                        self.active = Snapshot::default();
                        self.saved = Snapshot::default();
                        self.incomplete.clear();
                        self.state.clear();
                    }
                }
                return Ok(());
            }
            match self.step {
                2 => self.active.push(line)?,
                3 => self.saved.push(line)?,
                4 => {
                    let v = unescape(line)?;
                    if self.incomplete.len() + v.len() > MAX_LINE {
                        return Err("tmux pending sequence limit");
                    }
                    self.incomplete.extend(v);
                }
                5 => {
                    if !self.state.is_empty() || line.len() > 512 {
                        return Err("tmux state invalid");
                    }
                    self.state.extend_from_slice(line);
                }
                _ => {}
            }
            return Ok(());
        }
        if let Some(guard) = line.strip_prefix(b"%begin ") {
            let fields: Vec<_> = guard.split(|b| *b == b' ').collect();
            if fields.len() != 3
                || fields
                    .iter()
                    .any(|p| p.is_empty() || p.len() > 20 || !p.iter().all(u8::is_ascii_digit))
            {
                return Err("tmux guard invalid");
            }
            self.guard = Some(guard.to_vec());
        } else if let Some(value) = line.strip_prefix(b"%output ") {
            // Bootstrap commands run in one tmux command queue. Output preceding
            // its snapshot is already represented there and must not be replayed.
            if self.ready() {
                let (pane, value) = split_once(value, b' ').ok_or("tmux output invalid")?;
                if pane == self.pane {
                    out.extend(unescape(value)?);
                }
            }
        } else if line.starts_with(b"%error ") {
            return Err("tmux command failed");
        }
        Ok(())
    }
    fn restore(&mut self, out: &mut Vec<u8>) -> Result<(), &'static str> {
        let s = std::str::from_utf8(&self.state).map_err(|_| "tmux state invalid")?;
        let fields: Vec<_> = s.split_whitespace().collect();
        if fields.len() != 18 || fields[0] != "YUJI_STATE" {
            return Err("tmux state invalid");
        }
        let pane = fields[1].strip_prefix('%').ok_or("tmux pane invalid")?;
        if pane.is_empty() || pane.len() > 20 || !pane.bytes().all(|b| b.is_ascii_digit()) {
            return Err("tmux pane invalid");
        }
        let flags: Vec<u32> = fields[2..]
            .iter()
            .map(|v| v.parse::<u32>().map_err(|_| "tmux state invalid"))
            .collect::<Result<_, _>>()?;
        let (x, y, old_x, old_y) = (flags[0], flags[1], flags[2], flags[3]);
        if x > self.cols
            || y >= self.rows
            || (flags[4] == 1 && (old_x > 300 || old_y > 120))
            || flags[4..].iter().any(|n| *n > 1)
        {
            return Err("tmux state bounds");
        }
        self.pane = fields[1].as_bytes().to_vec();
        out.extend_from_slice(b"\x1bc");
        if flags[4] == 1 {
            self.saved.paint(out);
            out.extend(
                format!(
                    "\x1b[0m\x1b[{};{}H\x1b[?1049h",
                    old_y.min(self.rows - 1) + 1,
                    old_x.min(self.cols - 1) + 1
                )
                .as_bytes(),
            );
        }
        self.active.paint(out);
        out.extend(format!("\x1b[0m\x1b[{};{}H", y + 1, x.min(self.cols - 1) + 1).as_bytes());
        // Restore input and mouse modes for shells and full-screen applications.
        for (mode, enabled) in [
            (25, flags[5]),
            (7, flags[7]),
            (1, flags[8]),
            (1000, flags[10]),
            (1002, flags[11]),
            (1003, flags[12]),
            (1005, flags[13]),
            (1006, flags[14]),
            (2004, flags[15]),
        ] {
            out.extend(format!("\x1b[?{mode}{}", if enabled == 1 { 'h' } else { 'l' }).as_bytes());
        }
        out.extend(format!("\x1b[4{}", if flags[6] == 1 { 'h' } else { 'l' }).as_bytes());
        out.extend_from_slice(if flags[9] == 1 { b"\x1b=" } else { b"\x1b>" });
        out.extend_from_slice(&self.incomplete);
        Ok(())
    }
}
fn split_once(value: &[u8], byte: u8) -> Option<(&[u8], &[u8])> {
    let p = value.iter().position(|b| *b == byte)?;
    Some((&value[..p], &value[p + 1..]))
}
fn unescape(value: &[u8]) -> Result<Vec<u8>, &'static str> {
    decode(value, false)
}
fn decode(value: &[u8], capture: bool) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::with_capacity(value.len());
    let mut i = 0;
    while i < value.len() {
        if value[i] == b'\\' {
            // capture-pane -C quotes a literal backslash as \\, while
            // %output and capture-pane -P -C encode it as octal \\134.
            if capture && value.get(i + 1) == Some(&b'\\') {
                out.push(b'\\');
                i += 2;
                continue;
            }
            let oct = value.get(i + 1..i + 4).ok_or("tmux escape truncated")?;
            if oct[0] > b'3' || !oct.iter().all(|b| (b'0'..=b'7').contains(b)) {
                return Err("tmux escape invalid");
            }
            out.push((oct[0] - b'0') * 64 + (oct[1] - b'0') * 8 + oct[2] - b'0');
            i += 4;
        } else {
            out.push(value[i]);
            i += 1;
        }
    }
    Ok(out)
}
pub fn input(id: &str, bytes: &[u8]) -> Vec<u8> {
    assert!(crate::retained::valid(id));
    let mut out = String::with_capacity(bytes.len() * 3 + 256);
    for chunk in bytes.chunks(1024) {
        write!(out, "send-keys -H -t 'yuji_{id}:0.0'").unwrap();
        for byte in chunk {
            write!(out, " {byte:02x}").unwrap();
        }
        out.push('\n');
    }
    out.into_bytes()
}
pub fn resize(cols: u32, rows: u32) -> Vec<u8> {
    format!(
        "refresh-client -C {},{}\n",
        cols.clamp(20, 300),
        rows.clamp(5, 120)
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn init() -> Vec<u8> {
        let mut v = Vec::new();
        for (i, body) in [
            "",
            "",
            r"first\134literal",
            "",
            "",
            "YUJI_STATE %0 3 0 0 0 0 1 0 1 0 0 0 0 0 0 0 1",
        ]
        .iter()
        .enumerate()
        {
            v.extend(format!("%begin 123 {} 0\n", i + 1).as_bytes());
            if !body.is_empty() {
                v.extend(body.as_bytes());
                v.push(b'\n');
            }
            v.extend(format!("%end 123 {} 0\n", i + 1).as_bytes());
        }
        v
    }
    #[test]
    fn byte_fragmentation_and_escaped_output_preserve_all_bytes() {
        let mut parser = Stream::new(80, 24);
        let mut raw = init();
        raw.extend_from_slice(b"%output %0 line\\015\\012\\134\\033[31m\xe4\xb8\xad\xe6\x96\x87\n");
        let mut out = Vec::new();
        for b in raw {
            out.extend(parser.feed(&[b]).unwrap());
        }
        assert!(parser.ready());
        assert!(out.ends_with(b"line\r\n\\\x1b[31m\xe4\xb8\xad\xe6\x96\x87"));
        assert!(out.windows(13).any(|w| w == b"first\\literal"));
    }
    #[test]
    fn unset_saved_cursor_is_valid_outside_alternate_screen() {
        let init = String::from_utf8(init()).unwrap().replace(
            "YUJI_STATE %0 3 0 0 0 0",
            "YUJI_STATE %0 3 0 4294967295 4294967295 0",
        );
        let mut p = Stream::new(80, 24);
        assert!(!p.feed(init.as_bytes()).unwrap().is_empty());
        assert!(p.ready());
    }
    #[test]
    fn command_like_pane_output_cannot_escape_its_notification() {
        let mut p = Stream::new(80, 24);
        p.feed(&init()).unwrap();
        let b = p
            .feed(b"%output %0 \\045end 123 9 1\\012%begin 456 1 0\\012\n")
            .unwrap();
        assert_eq!(b, b"%end 123 9 1\n%begin 456 1 0\n");
        assert!(p.guard.is_none());
        assert!(p.feed(b"%output %99 secret\n").unwrap().is_empty());
    }
    #[test]
    fn input_is_hex_only_and_never_interpolates_shell_or_tmux_commands() {
        let id = "a".repeat(64);
        let payload = b"\n; kill-server\n$(id)\\\x00\xff";
        let s = String::from_utf8(input(&id, payload)).unwrap();
        assert_eq!(s.lines().count(), 1);
        assert!(!s.contains("kill-server"));
        assert!(!s.contains("$(id)"));
        let bytes: Vec<u8> = s
            .trim()
            .split(' ')
            .skip(4)
            .map(|v| u8::from_str_radix(v, 16).unwrap())
            .collect();
        assert_eq!(bytes, payload);
    }
    #[test]
    fn malformed_or_unbounded_records_fail_closed() {
        for raw in [
            b"%begin x 2 0\n".as_slice(),
            b"%output %0 bad\\99x\n",
            b"%output %0 short\\0\n",
        ] {
            let mut p = Stream::new(80, 24);
            p.feed(&init()).unwrap();
            assert!(p.feed(raw).is_err());
        }
        let mut p = Stream::new(80, 24);
        assert!(p.feed(&vec![b'x'; MAX_LINE + 1]).is_err());
    }
    #[test]
    fn snapshot_backslashes_use_capture_escaping_not_output_escaping() {
        assert_eq!(decode(br"printf 'x\\n'", true).unwrap(), br"printf 'x\n'");
        assert_eq!(unescape(br"x\134n").unwrap(), br"x\n");
        assert!(unescape(br"x\\n").is_err());
    }
    #[test]
    fn retained_snapshot_is_bounded_by_bytes_and_lines() {
        let mut snapshot = Snapshot::default();
        for _ in 0..HISTORY + 200 {
            snapshot.push(b"x").unwrap();
        }
        assert_eq!(snapshot.lines.len(), HISTORY + 120);
        for _ in 0..200 {
            snapshot.push(&vec![b'y'; 32768]).unwrap();
        }
        assert!(snapshot.bytes <= MAX_SNAPSHOT);
        assert!(snapshot.lines.back().unwrap().iter().all(|b| *b == b'y'));
    }
}
