//! A reader of WOFF2 fonts without transformed tables (fonts with CFF
//! outlines, like the embedded math font), which rebuilds the plain
//! OpenType file that font libraries read.

use std::io::Read;

/// Tags of the known tables, by their index in the WOFF2 table directory.
const KNOWN_TAGS: [&[u8; 4]; 63] = [
    b"cmap", b"head", b"hhea", b"hmtx", b"maxp", b"name", b"OS/2", b"post", b"cvt ", b"fpgm", b"glyf", b"loca",
    b"prep", b"CFF ", b"VORG", b"EBDT", b"EBLC", b"gasp", b"hdmx", b"kern", b"LTSH", b"PCLT", b"VDMX", b"vhea",
    b"vmtx", b"BASE", b"GDEF", b"GPOS", b"GSUB", b"EBSC", b"JSTF", b"MATH", b"CBDT", b"CBLC", b"COLR", b"CPAL",
    b"SVG ", b"sbix", b"acnt", b"avar", b"bdat", b"bloc", b"bsln", b"cvar", b"fdsc", b"feat", b"fmtx", b"fvar",
    b"gvar", b"hsty", b"just", b"lcar", b"mort", b"morx", b"opbd", b"prop", b"trak", b"Zapf", b"Silf", b"Glat",
    b"Gloc", b"Feat", b"Sill",
];

/// Size of the WOFF2 header.
const HEADER_LEN: usize = 48;

/// The OpenType file packed in the WOFF2 file `data`, or `None` when it is
/// not WOFF2, is a collection, or has transformed tables.
pub fn decode(data: &[u8]) -> Option<Vec<u8>> {
    let mut reader = Reader { data, pos: 0 };
    if reader.take(4)? != b"wOF2" {
        return None;
    }
    let flavor = reader.u32()?;
    reader.pos = 12;
    let num_tables = usize::from(reader.u16()?);
    reader.pos = 20;
    let compressed_len = usize::try_from(reader.u32()?).ok()?;
    reader.pos = HEADER_LEN;
    let mut tables: Vec<([u8; 4], usize)> = Vec::with_capacity(num_tables);
    for _ in 0..num_tables {
        let flags = reader.u8()?;
        let tag: [u8; 4] = match flags & 0x3F {
            63 => reader.take(4)?.try_into().ok()?,
            index => *KNOWN_TAGS[usize::from(index)],
        };
        let version = flags >> 6;
        let length = reader.base128()?;
        // glyf and loca are transformed unless their version is 3, the
        // other tables unless it is 0.
        let transformed = if &tag == b"glyf" || &tag == b"loca" {
            version != 3
        } else {
            version != 0
        };
        if transformed {
            return None;
        }
        tables.push((tag, length));
    }
    if flavor == u32::from_be_bytes(*b"ttcf") {
        return None;
    }
    let stream = data.get(reader.pos..reader.pos.checked_add(compressed_len)?)?;
    let mut raw = Vec::new();
    brotli_decompressor::Decompressor::new(stream, 4096)
        .read_to_end(&mut raw)
        .ok()?;
    sfnt(flavor, &tables, &raw)
}

/// The OpenType file of the tables `tables` (tag, length), whose data
/// follow each other in `raw`.
fn sfnt(flavor: u32, tables: &[([u8; 4], usize)], raw: &[u8]) -> Option<Vec<u8>> {
    let mut sorted: Vec<([u8; 4], &[u8])> = Vec::with_capacity(tables.len());
    let mut pos: usize = 0;
    for &(tag, length) in tables {
        sorted.push((tag, raw.get(pos..pos.checked_add(length)?)?));
        pos += length;
    }
    sorted.sort_by_key(|(tag, _)| *tag);
    let count = u16::try_from(sorted.len()).ok()?;
    let entry_selector = 15 - count.max(1).leading_zeros() as u16;
    let search_range = (1u16 << entry_selector) * 16;
    let mut out = Vec::new();
    out.extend_from_slice(&flavor.to_be_bytes());
    for value in [count, search_range, entry_selector, count * 16 - search_range] {
        out.extend_from_slice(&value.to_be_bytes());
    }
    let mut offset = 12 + 16 * sorted.len();
    for (tag, table) in &sorted {
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(table).to_be_bytes());
        out.extend_from_slice(&u32::try_from(offset).ok()?.to_be_bytes());
        out.extend_from_slice(&u32::try_from(table.len()).ok()?.to_be_bytes());
        offset += table.len().next_multiple_of(4);
    }
    for (_, table) in &sorted {
        out.extend_from_slice(table);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    Some(out)
}

/// The OpenType checksum of a table.
fn checksum(table: &[u8]) -> u32 {
    table.chunks(4).fold(0u32, |sum, chunk| {
        let mut word = [0u8; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// A cursor over big-endian data.
struct Reader<'a> {
    /// The data.
    data: &'a [u8],
    /// Position of the next byte.
    pos: usize,
}

impl<'a> Reader<'a> {
    /// The next `len` bytes.
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let bytes = self.data.get(self.pos..self.pos.checked_add(len)?)?;
        self.pos += len;
        Some(bytes)
    }

    /// The next byte.
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }

    /// The next 16-bit number.
    fn u16(&mut self) -> Option<u16> {
        Some(u16::from_be_bytes(self.take(2)?.try_into().ok()?))
    }

    /// The next 32-bit number.
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_be_bytes(self.take(4)?.try_into().ok()?))
    }

    /// The next number in WOFF2's variable-length encoding (`UIntBase128`).
    fn base128(&mut self) -> Option<usize> {
        let mut value: usize = 0;
        for _ in 0..5 {
            let byte = self.u8()?;
            value = value.checked_mul(128)? | usize::from(byte & 0x7F);
            if byte & 0x80 == 0 {
                return Some(value);
            }
        }
        None
    }
}
