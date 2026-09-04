"""Pin down the tone near 3.2 kHz: exact frequency, level, and cadence."""
import sys
import wave

import numpy as np

w = wave.open(sys.argv[1] if len(sys.argv) > 1 else "/tmp/beep.wav")
rate = w.getframerate()
n = w.getnframes()
x = np.frombuffer(w.readframes(n), dtype="<i2").astype(np.float64) / 32768.0

# Long window for frequency precision: 32768 pts = 1.46 Hz per bin.
N = 32768
win = np.hanning(N)
best = None
for off in range(0, len(x) - N, N // 2):
    seg = x[off:off + N] * win
    mag = np.abs(np.fft.rfft(seg))
    freqs = np.fft.rfftfreq(N, 1.0 / rate)
    band = (freqs > 2500) & (freqs < 4500)
    j = np.argmax(mag * band)
    if best is None or mag[j] > best[1]:
        best = (j, mag[j], freqs)

j, _, freqs = best
seg = x[:N] * win
mag = np.abs(np.fft.rfft(x[: N] * win))
# Parabolic interpolation around the peak for sub-bin accuracy.
seg_all = np.abs(np.fft.rfft(x[len(x) // 2 - N // 2: len(x) // 2 + N // 2] * win))
j2 = int(np.argmax(seg_all * ((freqs > 2500) & (freqs < 4500))))
a, b, c = (20 * np.log10(seg_all[j2 - 1] + 1e-12),
           20 * np.log10(seg_all[j2] + 1e-12),
           20 * np.log10(seg_all[j2 + 1] + 1e-12))
delta = 0.5 * (a - c) / (a - 2 * b + c) if (a - 2 * b + c) != 0 else 0.0
f_exact = freqs[j2] + delta * (rate / N)
print("EXACT FREQUENCY: %.1f Hz" % f_exact)

# Cadence: narrowband energy at that frequency over time vs broadband.
NB = 4096
hop = 512
k = int(round(f_exact / (rate / NB)))
wn = np.hanning(NB)
times, lvl = [], []
for i in range(0, len(x) - NB, hop):
    m = np.abs(np.fft.rfft(x[i:i + NB] * wn))
    tone = m[max(k - 1, 0):k + 2].max()
    floor = np.median(m)
    times.append(i / rate)
    lvl.append(20 * np.log10((tone / (floor + 1e-12)) + 1e-12))

lvl = np.array(lvl)
on = lvl > (lvl.max() - 12)
print("present in %.0f%% of the recording" % (100.0 * on.mean()))

# Runs of on/off, to tell a steady whine from a repeating beep.
runs, cur, start = [], on[0], 0
for i in range(1, len(on)):
    if on[i] != cur:
        runs.append((cur, (i - start) * hop / rate))
        cur, start = on[i], i
runs.append((cur, (len(on) - start) * hop / rate))
ons = [d for s, d in runs if s and d > 0.05]
offs = [d for s, d in runs if not s and d > 0.05]
if ons and offs:
    print("beeps: %d, mean on %.2fs, mean gap %.2fs" %
          (len(ons), float(np.mean(ons)), float(np.mean(offs))))
    print("period ~%.2fs (%.2f Hz repetition)" %
          (np.mean(ons) + np.mean(offs), 1.0 / (np.mean(ons) + np.mean(offs))))
else:
    print("continuous, not a repeating beep")

bar = "".join("#" if v else "." for v in on[::max(1, len(on) // 100)])
print("timeline (# = tone present):")
print(bar)
