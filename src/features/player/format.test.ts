import { describe, expect, it } from 'vitest';

import type { AudioProperties } from '@/types/player';

import { formatBadge, formatBytes, formatDb, formatSpan, formatStreamCompact, formatTime, isHiRes, khz } from './format';

const props = (over: Partial<AudioProperties>): AudioProperties => ({
  codec: 'FLAC',
  container: 'FLAC',
  lossless: true,
  sampleRate: 44100,
  bitDepth: 16,
  channels: 2,
  bitrateKbps: 900,
  durationMs: 180000,
  fileSize: 20_000_000,
  ...over,
});

describe('formatTime', () => {
  it('formats minutes and hours', () => {
    expect(formatTime(0)).toBe('0:00');
    expect(formatTime(134_900)).toBe('2:14');
    expect(formatTime(3_725_000)).toBe('1:02:05');
  });

  it('shows a placeholder for unknown durations', () => {
    expect(formatTime(null)).toBe('–:––');
    expect(formatTime(Number.NaN)).toBe('–:––');
  });
});

describe('format badges', () => {
  it('shows depth and rate for lossless files', () => {
    expect(formatBadge(props({ bitDepth: 24, sampleRate: 96000 }))).toBe('FLAC 24/96');
    expect(formatBadge(props({}))).toBe('FLAC 16/44.1');
    expect(formatBadge(props({ codec: 'ALAC', container: 'MP4', bitDepth: 24, sampleRate: 192000 }))).toBe('ALAC 24/192');
  });

  it('names PCM after its container', () => {
    expect(formatBadge(props({ codec: 'PCM', container: 'WAV', bitDepth: 24, sampleRate: 48000 }))).toBe('WAV 24/48');
  });

  it('shows bitrate for lossy files', () => {
    expect(formatBadge(props({ codec: 'MP3', lossless: false, bitDepth: null, bitrateKbps: 320 }))).toBe('MP3 320');
    expect(formatBadge(props({ codec: 'Opus', lossless: false, bitDepth: null, bitrateKbps: 160 }))).toBe('OPUS 160');
  });

  it('reserves the accent for 24-bit or high-rate material', () => {
    expect(isHiRes(props({}))).toBe(false);
    expect(isHiRes(props({ bitDepth: 24, sampleRate: 48000 }))).toBe(true);
    expect(isHiRes(props({ bitDepth: 16, sampleRate: 88200 }))).toBe(true);
  });
});

describe('readouts', () => {
  it('formats sample rates, sizes and spans', () => {
    expect(khz(44100)).toBe('44.1');
    expect(khz(96000)).toBe('96');
    expect(formatBytes(132_400_000)).toBe('132.4 MB');
    expect(formatBytes(1_280_000_000)).toBe('1.28 GB');
    expect(formatSpan(45_000)).toBe('45 s');
    expect(formatSpan(11_520_000)).toBe('3 h 12 min');
  });

  it('uses a true minus sign for gains', () => {
    expect(formatDb(-6.2)).toBe('−6.2 dB');
    expect(formatDb(1.5)).toBe('+1.5 dB');
    expect(formatDb(0)).toBe('0.0 dB');
    expect(formatDb(null)).toBe('−∞ dB');
  });

  it('abbreviates stream formats for signal-chain nodes', () => {
    expect(formatStreamCompact({ sampleFormat: 'S24LE', bits: 24, float: false, sampleRate: 96000, channels: 2 })).toBe('24/96');
    expect(formatStreamCompact({ sampleFormat: 'F32LE', bits: 32, float: true, sampleRate: 44100, channels: 2 })).toBe('32f/44.1');
  });
});
