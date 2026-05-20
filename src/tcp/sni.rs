use anyhow::Context;
use tokio::io::AsyncReadExt;
use tokio::net::TcpStream;

const TLS_HANDSHAKE_RECORD: u8 = 22;
const CLIENT_HELLO: u8 = 1;
const SERVER_NAME_EXTENSION: u16 = 0;
const HOST_NAME: u8 = 0;
const TLS_RECORD_HEADER_LEN: usize = 5;
const HANDSHAKE_HEADER_LEN: usize = 4;
const MAX_CLIENT_HELLO_BYTES: usize = 16 * 1024;

pub async fn read_client_hello_prefix(
    stream: &mut TcpStream,
) -> anyhow::Result<(Vec<u8>, Option<String>)> {
    let mut prefix = Vec::with_capacity(TLS_RECORD_HEADER_LEN);
    loop {
        read_record(stream, &mut prefix).await?;
        match parse_client_hello_sni(&prefix) {
            Ok(sni) => return Ok((prefix, sni)),
            Err(error)
                if error.to_string().contains("incomplete")
                    && prefix.len() < MAX_CLIENT_HELLO_BYTES => {}
            Err(error) => return Err(error),
        }
    }
}

pub fn parse_client_hello_sni(input: &[u8]) -> anyhow::Result<Option<String>> {
    let handshake = client_hello_handshake(input)?;
    if handshake.is_empty() {
        return Ok(None);
    }
    if handshake[0] != CLIENT_HELLO {
        return Ok(None);
    }
    let handshake_len = read_u24(&handshake[1..4]);
    let body_start = HANDSHAKE_HEADER_LEN;
    let body_end = body_start
        .checked_add(handshake_len)
        .context("TLS handshake length overflow")?;
    if body_end > handshake.len() {
        anyhow::bail!("TLS ClientHello handshake is incomplete");
    }
    parse_client_hello_body_sni(&handshake[body_start..body_end])
}

async fn read_record(stream: &mut TcpStream, prefix: &mut Vec<u8>) -> anyhow::Result<()> {
    let mut header = [0_u8; TLS_RECORD_HEADER_LEN];
    read_exact_bounded(stream, &mut header).await?;
    let record_len = u16::from_be_bytes([header[3], header[4]]) as usize;
    let total_len = prefix
        .len()
        .checked_add(TLS_RECORD_HEADER_LEN)
        .and_then(|len| len.checked_add(record_len))
        .context("TLS ClientHello length overflow")?;
    if total_len > MAX_CLIENT_HELLO_BYTES {
        anyhow::bail!("TLS ClientHello exceeds {MAX_CLIENT_HELLO_BYTES} byte limit");
    }

    prefix.extend_from_slice(&header);
    let start = prefix.len();
    prefix.resize(start + record_len, 0);
    read_exact_bounded(stream, &mut prefix[start..]).await?;
    Ok(())
}

async fn read_exact_bounded(stream: &mut TcpStream, buf: &mut [u8]) -> anyhow::Result<()> {
    let mut offset = 0;
    while offset < buf.len() {
        let read = stream.read(&mut buf[offset..]).await?;
        if read == 0 {
            anyhow::bail!("connection closed before TLS ClientHello completed");
        }
        offset += read;
    }
    Ok(())
}

fn client_hello_handshake(input: &[u8]) -> anyhow::Result<Vec<u8>> {
    let mut offset = 0;
    let mut handshake = Vec::new();
    while offset < input.len() {
        if input.len() - offset < TLS_RECORD_HEADER_LEN {
            anyhow::bail!("TLS record header is incomplete");
        }
        let record_type = input[offset];
        let record_len = u16::from_be_bytes([input[offset + 3], input[offset + 4]]) as usize;
        offset += TLS_RECORD_HEADER_LEN;
        let record_end = offset
            .checked_add(record_len)
            .context("TLS record length overflow")?;
        if record_end > input.len() {
            anyhow::bail!("TLS record is incomplete");
        }
        if record_type != TLS_HANDSHAKE_RECORD {
            return if handshake.is_empty() {
                Ok(Vec::new())
            } else {
                anyhow::bail!("TLS ClientHello handshake is incomplete")
            };
        }
        handshake.extend_from_slice(&input[offset..record_end]);
        if handshake.len() >= HANDSHAKE_HEADER_LEN {
            if handshake[0] != CLIENT_HELLO {
                return Ok(handshake);
            }
            let handshake_len = read_u24(&handshake[1..4]);
            let total_len = HANDSHAKE_HEADER_LEN
                .checked_add(handshake_len)
                .context("TLS handshake length overflow")?;
            if handshake.len() >= total_len {
                handshake.truncate(total_len);
                return Ok(handshake);
            }
        }
        offset = record_end;
    }
    anyhow::bail!("TLS ClientHello handshake is incomplete")
}

fn parse_client_hello_body_sni(body: &[u8]) -> anyhow::Result<Option<String>> {
    let mut cursor = Cursor::new(body);
    cursor.take(2)?; // legacy_version
    cursor.take(32)?; // random
    let session_id_len = cursor.u8()? as usize;
    cursor.take(session_id_len)?;
    let cipher_suites_len = cursor.u16()? as usize;
    cursor.take(cipher_suites_len)?;
    let compression_methods_len = cursor.u8()? as usize;
    cursor.take(compression_methods_len)?;
    if cursor.remaining() == 0 {
        return Ok(None);
    }
    let extensions_len = cursor.u16()? as usize;
    let extensions = cursor.take(extensions_len)?;
    parse_extensions_sni(extensions)
}

fn parse_extensions_sni(mut extensions: &[u8]) -> anyhow::Result<Option<String>> {
    while !extensions.is_empty() {
        if extensions.len() < 4 {
            anyhow::bail!("TLS extension header is incomplete");
        }
        let extension_type = u16::from_be_bytes([extensions[0], extensions[1]]);
        let extension_len = u16::from_be_bytes([extensions[2], extensions[3]]) as usize;
        extensions = &extensions[4..];
        if extension_len > extensions.len() {
            anyhow::bail!("TLS extension body is incomplete");
        }
        let extension = &extensions[..extension_len];
        extensions = &extensions[extension_len..];
        if extension_type == SERVER_NAME_EXTENSION {
            return parse_server_name_extension(extension);
        }
    }
    Ok(None)
}

fn parse_server_name_extension(extension: &[u8]) -> anyhow::Result<Option<String>> {
    let mut cursor = Cursor::new(extension);
    let list_len = cursor.u16()? as usize;
    let mut names = cursor.take(list_len)?;
    while !names.is_empty() {
        if names.len() < 3 {
            anyhow::bail!("TLS server_name entry is incomplete");
        }
        let name_type = names[0];
        let name_len = u16::from_be_bytes([names[1], names[2]]) as usize;
        names = &names[3..];
        if name_len > names.len() {
            anyhow::bail!("TLS server_name hostname is incomplete");
        }
        let name = &names[..name_len];
        names = &names[name_len..];
        if name_type == HOST_NAME {
            let host = std::str::from_utf8(name)
                .context("TLS server_name is not valid UTF-8")?
                .to_ascii_lowercase();
            return Ok(Some(host));
        }
    }
    Ok(None)
}

fn read_u24(bytes: &[u8]) -> usize {
    ((bytes[0] as usize) << 16) | ((bytes[1] as usize) << 8) | bytes[2] as usize
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.offset)
    }

    fn take(&mut self, len: usize) -> anyhow::Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .context("TLS cursor overflow")?;
        if end > self.bytes.len() {
            anyhow::bail!("TLS ClientHello field is incomplete");
        }
        let bytes = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }

    fn u8(&mut self) -> anyhow::Result<u8> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> anyhow::Result<u16> {
        let bytes = self.take(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_client_hello_sni;

    #[test]
    fn parses_client_hello_sni() {
        let hello = client_hello("app.example.com");

        let sni = parse_client_hello_sni(&hello).unwrap();

        assert_eq!(sni.as_deref(), Some("app.example.com"));
    }

    #[test]
    fn parses_client_hello_sni_across_records() {
        let hello = client_hello("app.example.com");
        let first_payload = 8;
        let first = tls_record(&hello[5..5 + first_payload]);
        let second = tls_record(&hello[5 + first_payload..]);
        let mut split = first;
        split.extend_from_slice(&second);

        let sni = parse_client_hello_sni(&split).unwrap();

        assert_eq!(sni.as_deref(), Some("app.example.com"));
    }

    #[test]
    fn lowercases_sni_without_trimming_trailing_dot() {
        let hello = client_hello("App.Example.Com.");

        let sni = parse_client_hello_sni(&hello).unwrap();

        assert_eq!(sni.as_deref(), Some("app.example.com."));
    }

    fn client_hello(host: &str) -> Vec<u8> {
        let host = host.as_bytes();
        let mut server_name = Vec::new();
        server_name.extend_from_slice(&((host.len() + 3) as u16).to_be_bytes());
        server_name.push(0);
        server_name.extend_from_slice(&(host.len() as u16).to_be_bytes());
        server_name.extend_from_slice(host);

        let mut extensions = Vec::new();
        extensions.extend_from_slice(&0_u16.to_be_bytes());
        extensions.extend_from_slice(&(server_name.len() as u16).to_be_bytes());
        extensions.extend_from_slice(&server_name);

        let mut body = Vec::new();
        body.extend_from_slice(&[0x03, 0x03]);
        body.extend_from_slice(&[0_u8; 32]);
        body.push(0);
        body.extend_from_slice(&2_u16.to_be_bytes());
        body.extend_from_slice(&[0x13, 0x01]);
        body.push(1);
        body.push(0);
        body.extend_from_slice(&(extensions.len() as u16).to_be_bytes());
        body.extend_from_slice(&extensions);

        let mut handshake = Vec::new();
        handshake.push(1);
        handshake.push(((body.len() >> 16) & 0xff) as u8);
        handshake.push(((body.len() >> 8) & 0xff) as u8);
        handshake.push((body.len() & 0xff) as u8);
        handshake.extend_from_slice(&body);

        let mut record = Vec::new();
        record.push(22);
        record.extend_from_slice(&[0x03, 0x01]);
        record.extend_from_slice(&(handshake.len() as u16).to_be_bytes());
        record.extend_from_slice(&handshake);
        record
    }

    fn tls_record(payload: &[u8]) -> Vec<u8> {
        let mut record = Vec::new();
        record.push(22);
        record.extend_from_slice(&[0x03, 0x01]);
        record.extend_from_slice(&(payload.len() as u16).to_be_bytes());
        record.extend_from_slice(payload);
        record
    }
}
