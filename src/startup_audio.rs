//! One finite PCM buffer on the Windows mixer. No decoder, thread, global-volume
//! changes or playback loop. The intro owns it and stops it on dismissal/drop.
use windows::{Win32::Media::Audio::*, core::PSTR};
pub const WAV: &[u8] = include_bytes!("../assets/intro/ice-startup.wav");
const HEADER_SIZE: u32 = std::mem::size_of::<WAVEHDR>() as u32;

fn pcm(wav: &[u8]) -> Option<&[u8]> {
    if wav.len() > 2 * 1024 * 1024 || wav.get(..4)? != b"RIFF" || wav.get(8..12)? != b"WAVE" {
        return None;
    }
    let declared = u32::from_le_bytes(wav.get(4..8)?.try_into().ok()?) as usize;
    if declared.checked_add(8)? != wav.len() {
        return None;
    }
    let mut position = 12usize;
    let mut valid = false;
    let mut data = None;
    while position < wav.len() {
        let kind = wav.get(position..position + 4)?;
        let length =
            u32::from_le_bytes(wav.get(position + 4..position + 8)?.try_into().ok()?) as usize;
        let start = position.checked_add(8)?;
        let end = start.checked_add(length)?;
        let chunk = wav.get(start..end)?;
        if kind == b"fmt " {
            valid = chunk.get(0..2)? == 1u16.to_le_bytes()
                && chunk.get(2..4)? == 2u16.to_le_bytes()
                && chunk.get(4..8)? == 44100u32.to_le_bytes()
                && chunk.get(8..12)? == 176400u32.to_le_bytes()
                && chunk.get(12..14)? == 4u16.to_le_bytes()
                && chunk.get(14..16)? == 16u16.to_le_bytes();
        } else if kind == b"data" {
            data = Some(chunk);
        }
        position = end.checked_add(length % 2)?;
    }
    data.filter(|data| valid && !data.is_empty() && data.len() % 4 == 0)
}

pub struct Playback {
    handle: HWAVEOUT,
    // Both allocations remain at stable addresses until the driver releases them.
    header: Option<Box<WAVEHDR>>,
    samples: Box<[u8]>,
    prepared: bool,
    running: bool,
}
impl Playback {
    pub fn open() -> Option<Self> {
        let mut samples = pcm(WAV)?.to_vec().into_boxed_slice();
        let format = WAVEFORMATEX {
            wFormatTag: 1,
            nChannels: 2,
            nSamplesPerSec: 44100,
            nAvgBytesPerSec: 176400,
            nBlockAlign: 4,
            wBitsPerSample: 16,
            cbSize: 0,
        };
        let mut handle = HWAVEOUT::default();
        if unsafe {
            waveOutOpen(
                Some(&mut handle),
                WAVE_MAPPER,
                &format,
                None,
                None,
                CALLBACK_NULL,
            )
        } != 0
        {
            return None;
        }
        let header = Box::new(WAVEHDR {
            lpData: PSTR(samples.as_mut_ptr()),
            dwBufferLength: samples.len() as u32,
            ..Default::default()
        });
        let mut playback = Self {
            handle,
            header: Some(header),
            samples,
            prepared: false,
            running: false,
        };
        let header = playback.header.as_mut().unwrap().as_mut();
        if unsafe { waveOutPrepareHeader(handle, header, HEADER_SIZE) } != 0 {
            return None;
        }
        playback.prepared = true;
        // Queue while paused, so sample zero and the visual clock start together.
        if unsafe { waveOutPause(handle) } != 0
            || unsafe { waveOutWrite(handle, header, HEADER_SIZE) } != 0
        {
            return None;
        }
        Some(playback)
    }
    pub fn set_running(&mut self, running: bool) {
        if running == self.running {
            return;
        }
        let status = unsafe {
            if running {
                waveOutRestart(self.handle)
            } else {
                waveOutPause(self.handle)
            }
        };
        if status == 0 {
            self.running = running;
        }
    }
    #[cfg(test)]
    fn sample_position(&self) -> Option<u32> {
        use windows::Win32::Media::{MMTIME, TIME_BYTES, TIME_MS, TIME_SAMPLES};
        let mut time = MMTIME {
            wType: TIME_SAMPLES,
            ..Default::default()
        };
        if unsafe {
            waveOutGetPosition(self.handle, &mut time, std::mem::size_of::<MMTIME>() as u32)
        } != 0
        {
            return None;
        }
        unsafe {
            match time.wType {
                TIME_SAMPLES => Some(time.u.sample),
                TIME_BYTES => Some(time.u.cb / 4),
                TIME_MS => Some(time.u.ms * 441 / 10),
                _ => None,
            }
        }
    }
}
impl Drop for Playback {
    fn drop(&mut self) {
        unsafe {
            waveOutReset(self.handle);
            if let Some(mut header) = self.header.take()
                && self.prepared
                && waveOutUnprepareHeader(self.handle, header.as_mut(), HEADER_SIZE) != 0
            {
                // A broken driver may still retain these pointers. Keep that memory
                // alive on its error path instead of freeing a driver-owned buffer.
                Box::leak(header);
                Box::leak(std::mem::take(&mut self.samples));
            }
            waveOutClose(self.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cue_has_exact_duration_quiet_edges_and_impact_at_the_visual_strike() {
        let data = pcm(WAV).unwrap();
        assert_eq!(data.len(), 44100 * 4 * 25 / 10);
        let samples: Vec<i16> = data
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        assert!(samples[..100].iter().all(|n| n.unsigned_abs() < 20));
        assert!(
            samples[samples.len() - 2000..]
                .iter()
                .all(|n| n.unsigned_abs() < 20)
        );
        let (index, peak) = samples
            .iter()
            .enumerate()
            .max_by_key(|(_, v)| v.unsigned_abs())
            .unwrap();
        assert!((14000..14500).contains(&peak.unsigned_abs()));
        let peak_seconds = index as f32 / (44100. * 2.);
        assert!((1.10..1.26).contains(&peak_seconds));
        let mut damaged = WAV.to_vec();
        damaged[0] = 0;
        assert!(pcm(&damaged).is_none());
        assert!(pcm(&WAV[..WAV.len() - 1]).is_none());
    }
    #[test]
    #[ignore = "Plays the startup sound on the current Windows audio output"]
    fn real_output_pauses_resumes_and_can_be_reopened_after_drop() {
        use std::{thread::sleep, time::Duration};
        let mut sound = Playback::open().expect("Windows audio output");
        sound.set_running(true);
        sleep(Duration::from_millis(250));
        let at = sound.sample_position().unwrap();
        assert!(at > 3000);
        sound.set_running(false);
        sleep(Duration::from_millis(70));
        let paused = sound.sample_position().unwrap();
        sleep(Duration::from_millis(180));
        assert!(sound.sample_position().unwrap().abs_diff(paused) <= 441);
        sound.set_running(true);
        sleep(Duration::from_millis(240));
        assert!(sound.sample_position().unwrap() > paused + 3000);
        sleep(Duration::from_millis(2050));
        drop(sound);
        let reopened = Playback::open().expect("Output released after playback");
        drop(reopened);
    }
}
