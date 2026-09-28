// =============================================================================
// Buffer para una trama Ethernet de tamaño fijo
// =============================================================================

use crate::eth::MAX_FRAME_SIZE;

#[derive(Clone, Copy)]
pub struct Frame {
    pub data: [u8; MAX_FRAME_SIZE],
    pub len: usize,
}
    
impl Frame {
    pub fn new() -> Self {
        Frame { data: [0u8; MAX_FRAME_SIZE], len: 0 }
    }
    
    pub fn as_slice(&self) -> &[u8] {
        &self.data[..self.len]
    }
}

