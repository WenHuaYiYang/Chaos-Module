//! 通透背景的独立像素处理，不借用或保存系统快照缓冲。

pub const SOURCE_WIDTH: usize = 336;
pub const SOURCE_HEIGHT: usize = 480;
pub const WIDTH: usize = SOURCE_WIDTH / 4;
pub const HEIGHT: usize = SOURCE_HEIGHT / 4;
pub const CHANNELS: usize = 3;
pub const PIXEL_BYTES: usize = WIDTH * HEIGHT * CHANNELS;
pub const FULL_BYTES: usize = SOURCE_WIDTH * SOURCE_HEIGHT * CHANNELS;
pub const SCRATCH_BYTES: usize = HEIGHT * CHANNELS;
pub const MAX_BATCH_LINES: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelError {
    Dimensions,
    Stride,
    Source,
    Destination,
    Scratch,
}

/// 降采样到独立缓冲。输入为 RGB888 的三个颜色字节。
/// 所有长度先检查，失败不写目标；行尾填充不参与计算。
pub fn downsample(
    source: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    destination: &mut [u8],
) -> Result<(), PixelError> {
    if width != SOURCE_WIDTH || height != SOURCE_HEIGHT {
        return Err(PixelError::Dimensions);
    }
    if stride < SOURCE_WIDTH * CHANNELS {
        return Err(PixelError::Stride);
    }
    let required = stride.checked_mul(SOURCE_HEIGHT - 1)
        .and_then(|n| n.checked_add(SOURCE_WIDTH * CHANNELS))
        .ok_or(PixelError::Source)?;
    if source.len() < required {
        return Err(PixelError::Source);
    }
    if destination.len() < PIXEL_BYTES {
        return Err(PixelError::Destination);
    }
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            for channel in 0..CHANNELS {
                let mut sum = 0u32;
                for dy in 0..4 {
                    for dx in 0..4 {
                        sum += source[(y * 4 + dy) * stride
                            + (x * 4 + dx) * CHANNELS + channel] as u32;
                    }
                }
                destination[(y * WIDTH + x) * CHANNELS + channel] = (sum / 16) as u8;
            }
        }
    }
    Ok(())
}

/// 分批模糊只记录进度，不保存系统快照或像素指针。
pub struct BlurJob {
    phase: u8,
    line: usize,
}

impl BlurJob {
    pub const fn new() -> Self {
        Self { phase: 0, line: 0 }
    }

    pub fn is_done(&self) -> bool {
        self.phase == 4
    }

    /// 每次最多处理固定数量的行或列，交给已有 UI 调度消费。
    /// 失败不改变像素或进度；零预算不推进，完成后可重复调用。
    pub fn step(
        &mut self,
        destination: &mut [u8],
        scratch: &mut [u8],
        budget: usize,
    ) -> Result<usize, PixelError> {
        if destination.len() < PIXEL_BYTES {
            return Err(PixelError::Destination);
        }
        if scratch.len() < SCRATCH_BYTES {
            return Err(PixelError::Scratch);
        }
        let mut processed = 0;
        while processed < budget.min(MAX_BATCH_LINES) && !self.is_done() {
            if self.phase & 1 == 0 {
                let y = self.line;
                for x in 0..WIDTH {
                    for channel in 0..CHANNELS {
                        scratch[x * CHANNELS + channel] =
                            destination[(y * WIDTH + x) * CHANNELS + channel];
                    }
                }
                for x in 0..WIDTH {
                    for channel in 0..CHANNELS {
                        let mut sum = 0u32;
                        for offset in -2isize..=2 {
                            let at = (x as isize + offset).clamp(0, WIDTH as isize - 1) as usize;
                            sum += scratch[at * CHANNELS + channel] as u32;
                        }
                        destination[(y * WIDTH + x) * CHANNELS + channel] = (sum / 5) as u8;
                    }
                }
            } else {
                let x = self.line;
                for y in 0..HEIGHT {
                    for channel in 0..CHANNELS {
                        scratch[y * CHANNELS + channel] =
                            destination[(y * WIDTH + x) * CHANNELS + channel];
                    }
                }
                for y in 0..HEIGHT {
                    for channel in 0..CHANNELS {
                        let mut sum = 0u32;
                        for offset in -2isize..=2 {
                            let at = (y as isize + offset).clamp(0, HEIGHT as isize - 1) as usize;
                            sum += scratch[at * CHANNELS + channel] as u32;
                        }
                        destination[(y * WIDTH + x) * CHANNELS + channel] = (sum / 5) as u8;
                    }
                }
            }
            processed += 1;
            self.line += 1;
            let end = if self.phase & 1 == 0 { HEIGHT } else { WIDTH };
            if self.line == end {
                self.line = 0;
                self.phase += 1;
            }
        }
        Ok(processed)
    }
}

/// 同步入口用于独立转换；页面接入应先降采样、归还快照，再分批模糊。
pub fn prepare(
    source: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    destination: &mut [u8],
    scratch: &mut [u8],
    blur: bool,
) -> Result<(), PixelError> {
    // 提前检查模糊工作区，防止输入失败时已写入目标。
    if blur && scratch.len() < SCRATCH_BYTES {
        return Err(PixelError::Scratch);
    }
    downsample(source, width, height, stride, destination)?;
    if blur {
        let mut job = BlurJob::new();
        while !job.is_done() {
            job.step(destination, scratch, MAX_BATCH_LINES)?;
        }
    }
    Ok(())
}

/// 生成期间分批展开；后续绘制使用原尺寸图，不再执行全屏缩放。
pub fn expand_rows(source: &[u8], destination: &mut [u8], start: usize, budget: usize)
    -> Result<usize, PixelError> {
    if source.len() < PIXEL_BYTES { return Err(PixelError::Source); }
    if destination.len() < FULL_BYTES || start > SOURCE_HEIGHT {
        return Err(PixelError::Destination);
    }
    let end = (start + budget.min(MAX_BATCH_LINES)).min(SOURCE_HEIGHT);
    for y in start..end {
        let row = y / 4;
        let next_row = (row + 1).min(HEIGHT - 1);
        let dy = (y & 3) as u32;
        for x in 0..SOURCE_WIDTH {
            let column = x / 4;
            let next_column = (column + 1).min(WIDTH - 1);
            let dx = (x & 3) as u32;
            for channel in 0..CHANNELS {
                let a = source[(row * WIDTH + column) * CHANNELS + channel] as u32;
                let b = source[(row * WIDTH + next_column) * CHANNELS + channel] as u32;
                let c = source[(next_row * WIDTH + column) * CHANNELS + channel] as u32;
                let d = source[(next_row * WIDTH + next_column) * CHANNELS + channel] as u32;
                destination[(y * SOURCE_WIDTH + x) * CHANNELS + channel] =
                    ((a * (4 - dx) * (4 - dy) + b * dx * (4 - dy)
                      + c * (4 - dx) * dy + d * dx * dy) / 16) as u8;
            }
        }
    }
    Ok(end - start)
}
