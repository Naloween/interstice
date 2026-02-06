use crate::{error::IntersticeError, network::protocol::NetworkPacket};
use interstice_abi::{decode, encode};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn write_packet<W: AsyncWriteExt + Unpin>(
    writer: &mut W,
    packet: &NetworkPacket,
) -> Result<(), IntersticeError> {
    let bytes = encode(packet).map_err(|err| {
        IntersticeError::Internal(format!("Couldn't encode network packet: {}", err))
    })?;
    writer
        .write_u32(bytes.len() as u32)
        .await
        .map_err(|err| IntersticeError::Internal(format!("Failed to write packet: {err}")))?;
    writer
        .write_all(&bytes)
        .await
        .map_err(|err| IntersticeError::Internal(format!("Failed to write packet: {err}")))?;
    Ok(())
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
