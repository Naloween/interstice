use crate::{error::IntersticeError, network::protocol::NetworkPacket};
use interstice_abi::{decode, encode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Write one frame (u32 length, then the encoded packet) and flush it.
pub async fn write_packet<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    packet: &NetworkPacket,
) -> Result<(), IntersticeError> {
    write_packet_unflushed(writer, packet).await?;
    // A TLS stream may hold the last record back until flushed.
    writer
        .flush()
        .await
        .map_err(|err| IntersticeError::Internal(format!("Failed to write packet: {err}")))
}

/// Write one frame without flushing, for a writer that sends several
/// packets and then flushes once.
pub async fn write_packet_unflushed<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    packet: &NetworkPacket,
) -> Result<(), IntersticeError> {
    let bytes = encode(packet).map_err(|err| {
        IntersticeError::Internal(format!("Couldn't encode network packet: {}", err))
    })?;
    let mut frame = Vec::with_capacity(4 + bytes.len());
    frame.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    frame.extend_from_slice(&bytes);
    writer
        .write_all(&frame)
        .await
        .map_err(|err| IntersticeError::Internal(format!("Failed to write packet: {err}")))
}

pub async fn read_packet<R: AsyncReadExt + Unpin>(
    reader: &mut R,
) -> Result<NetworkPacket, IntersticeError> {
    let len = reader
        .read_u32()
        .await
        .map_err(|err| IntersticeError::Internal(format!("Failed to read packet: {err}")))?;
    let mut buf = vec![0u8; len as usize];
    reader
        .read_exact(&mut buf)
        .await
        .map_err(|err| IntersticeError::Internal(format!("Failed to read packet: {err}")))?;
    Ok(decode(&buf).map_err(|_err| IntersticeError::Internal("Failed to decode packet".into()))?)
}
