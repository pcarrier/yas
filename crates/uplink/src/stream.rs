//! Bounded length-prefixed Noise records with authenticated half-close.
use crate::{DatagramKeyMaterial, REKEY_INTERVAL, crypto_error, invalid};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use zeroize::{Zeroize, Zeroizing};

pub const MAX_PLAINTEXT: usize = 16 * 1024;
const MAX_CIPHERTEXT: usize = MAX_PLAINTEXT + 1 + 16;
const DATA: u8 = 0;
const FIN: u8 = 1;

/// Synchronous crypto over an asynchronous carrier. Buffering is bounded to
/// one record per direction. Dropping it drops its carrier; there are no tasks.
pub struct NoiseStream<S> {
    carrier: S,
    state: snow::TransportState,
    pub(crate) datagram_keys: DatagramKeyMaterial,
    header: [u8; 2],
    header_len: usize,
    cipher: Vec<u8>,
    cipher_len: usize,
    plaintext: Zeroizing<Vec<u8>>,
    plaintext_offset: usize,
    outgoing: Vec<u8>,
    outgoing_offset: usize,
    read_closed: bool,
    write_closed: bool,
    failed: bool,
}

impl<S> std::fmt::Debug for NoiseStream<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoiseStream").finish_non_exhaustive()
    }
}

impl<S> NoiseStream<S> {
    pub(crate) fn new(carrier: S, state: snow::TransportState) -> Self {
        Self {
            carrier,
            state,
            datagram_keys: DatagramKeyMaterial::new([0; 64]),
            header: [0; 2],
            header_len: 0,
            cipher: Vec::new(),
            cipher_len: 0,
            plaintext: Zeroizing::new(Vec::new()),
            plaintext_offset: 0,
            outgoing: Vec::new(),
            outgoing_offset: 0,
            read_closed: false,
            write_closed: false,
            failed: false,
        }
    }

    pub fn datagram_key_material(&self) -> DatagramKeyMaterial {
        self.datagram_keys.clone()
    }

    fn seal(&mut self, kind: u8, bytes: &[u8]) -> io::Result<()> {
        let mut plaintext = Zeroizing::new(Vec::with_capacity(bytes.len() + 1));
        plaintext.push(kind);
        plaintext.extend_from_slice(bytes);
        self.outgoing.resize(2 + plaintext.len() + 16, 0);
        let length = self
            .state
            .write_message(&plaintext, &mut self.outgoing[2..])
            .map_err(crypto_error)?;
        self.outgoing[..2].copy_from_slice(&(length as u16).to_be_bytes());
        self.outgoing_offset = 0;
        if self.state.sending_nonce().is_multiple_of(REKEY_INTERVAL) {
            self.state.rekey_outgoing();
        }
        Ok(())
    }

    fn open(&mut self) -> io::Result<()> {
        self.plaintext.resize(self.cipher.len(), 0);
        let len = self
            .state
            .read_message(&self.cipher, &mut self.plaintext)
            .map_err(crypto_error)?;
        self.plaintext.truncate(len);
        if self.state.receiving_nonce().is_multiple_of(REKEY_INTERVAL) {
            self.state.rekey_incoming();
        }
        match self.plaintext.first() {
            Some(&DATA) if len > 1 => self.plaintext_offset = 1,
            Some(&FIN) if len == 1 => {
                self.read_closed = true;
                self.plaintext.zeroize();
                self.plaintext.clear();
            }
            _ => return Err(invalid()),
        }
        self.header_len = 0;
        self.cipher.clear();
        self.cipher_len = 0;
        Ok(())
    }
}

impl<S: AsyncWrite + Unpin> NoiseStream<S> {
    fn drain(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.outgoing_offset < self.outgoing.len() {
            match ready!(
                Pin::new(&mut self.carrier).poll_write(cx, &self.outgoing[self.outgoing_offset..])
            ) {
                Ok(0) => {
                    self.failed = true;
                    return Poll::Ready(Err(io::ErrorKind::WriteZero.into()));
                }
                Ok(n) => self.outgoing_offset += n,
                Err(error) => {
                    self.failed = true;
                    return Poll::Ready(Err(error));
                }
            }
        }
        self.outgoing.clear();
        self.outgoing_offset = 0;
        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncRead + Unpin> NoiseStream<S> {
    fn read_record(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        while self.header_len < 2 {
            let mut bytes = ReadBuf::new(&mut self.header[self.header_len..]);
            ready!(Pin::new(&mut self.carrier).poll_read(cx, &mut bytes))?;
            if bytes.filled().is_empty() {
                return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
            }
            self.header_len += bytes.filled().len();
        }
        if self.cipher.is_empty() {
            let len = u16::from_be_bytes(self.header) as usize;
            if !(17..=MAX_CIPHERTEXT).contains(&len) {
                return Poll::Ready(Err(invalid()));
            }
            self.cipher.resize(len, 0);
        }
        while self.cipher_len < self.cipher.len() {
            let mut bytes = ReadBuf::new(&mut self.cipher[self.cipher_len..]);
            ready!(Pin::new(&mut self.carrier).poll_read(cx, &mut bytes))?;
            if bytes.filled().is_empty() {
                return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
            }
            self.cipher_len += bytes.filled().len();
        }
        Poll::Ready(self.open())
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for NoiseStream<S> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        out: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.failed {
            return Poll::Ready(Err(invalid()));
        }
        if out.remaining() == 0 || this.read_closed {
            return Poll::Ready(Ok(()));
        }
        if this.plaintext_offset >= this.plaintext.len() {
            this.plaintext.zeroize();
            this.plaintext.clear();
            this.plaintext_offset = 0;
            if let Err(error) = ready!(this.read_record(cx)) {
                this.failed = true;
                return Poll::Ready(Err(error));
            }
        }
        if !this.read_closed {
            let count = out
                .remaining()
                .min(this.plaintext.len() - this.plaintext_offset);
            out.put_slice(&this.plaintext[this.plaintext_offset..this.plaintext_offset + count]);
            this.plaintext_offset += count;
        }
        Poll::Ready(Ok(()))
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for NoiseStream<S> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if this.failed {
            return Poll::Ready(Err(invalid()));
        }
        if this.write_closed {
            return Poll::Ready(Err(io::ErrorKind::BrokenPipe.into()));
        }
        ready!(this.drain(cx))?;
        if bytes.is_empty() {
            return Poll::Ready(Ok(0));
        }
        let count = bytes.len().min(MAX_PLAINTEXT);
        if let Err(error) = this.seal(DATA, &bytes[..count]) {
            this.failed = true;
            return Poll::Ready(Err(error));
        }
        Poll::Ready(Ok(count))
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.failed {
            return Poll::Ready(Err(invalid()));
        }
        ready!(this.drain(cx))?;
        let result = ready!(Pin::new(&mut this.carrier).poll_flush(cx));
        if result.is_err() {
            this.failed = true;
        }
        Poll::Ready(result)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.failed {
            return Poll::Ready(Err(invalid()));
        }
        ready!(this.drain(cx))?;
        if !this.write_closed {
            if let Err(error) = this.seal(FIN, &[]) {
                this.failed = true;
                return Poll::Ready(Err(error));
            }
            this.write_closed = true;
        }
        ready!(this.drain(cx))?;
        let result = ready!(Pin::new(&mut this.carrier).poll_shutdown(cx));
        if result.is_err() {
            this.failed = true;
        }
        Poll::Ready(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reliable_records_rekey_without_resetting_nonces() {
        let protocol = crate::NOISE_PROTOCOL.parse().unwrap();
        let client = [7; 32];
        let server = x25519_dalek::StaticSecret::from([8; 32]);
        let pin = x25519_dalek::PublicKey::from(&server);
        let mut a = snow::Builder::new(protocol)
            .local_private_key(&client)
            .unwrap()
            .remote_public_key(pin.as_bytes())
            .unwrap()
            .build_initiator()
            .unwrap();
        let mut b = snow::Builder::new(crate::NOISE_PROTOCOL.parse().unwrap())
            .local_private_key(&server.to_bytes())
            .unwrap()
            .build_responder()
            .unwrap();
        let mut message = [0; 256];
        let mut plaintext = [0; 256];
        let n = a.write_message(&[], &mut message).unwrap();
        b.read_message(&message[..n], &mut plaintext).unwrap();
        let n = b.write_message(&[], &mut message).unwrap();
        a.read_message(&message[..n], &mut plaintext).unwrap();
        let mut sender = NoiseStream::new((), a.into_transport_mode().unwrap());
        let mut receiver = NoiseStream::new((), b.into_transport_mode().unwrap());
        // Snow deliberately exposes no setter for sending nonces. Advance
        // through the real boundary; skip decrypting the discarded records.
        for _ in 0..REKEY_INTERVAL - 1 {
            sender.seal(DATA, b"x").unwrap();
        }
        receiver.state.set_receiving_nonce(REKEY_INTERVAL - 1);
        for counter in REKEY_INTERVAL - 1..=REKEY_INTERVAL + 1 {
            sender.seal(DATA, b"boundary").unwrap();
            receiver.cipher = sender.outgoing[2..].to_vec();
            receiver.open().unwrap();
            assert_eq!(&receiver.plaintext[1..], b"boundary");
            assert_eq!(sender.state.sending_nonce(), counter + 1);
            assert_eq!(receiver.state.receiving_nonce(), counter + 1);
        }
    }
}
