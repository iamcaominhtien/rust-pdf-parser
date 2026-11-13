# Sounds of the friendly mode

`page-turn.mp3` is played when a page of the book view turns.

- Source: "Page Turn" by davidbain, Freesound sound 136778,
  <https://freesound.org/people/davidbain/sounds/136778/>
- License: Creative Commons 0 (public domain dedication),
  <https://creativecommons.org/publicdomain/zero/1.0/>. No attribution is
  required; it is given here as a courtesy.
- Taken from the high-quality preview of the sound and processed with
  ffmpeg: the quiet start and end trimmed (0.34 s to 1.52 s), high-passed at
  90 Hz, raised by 1.6 dB, faded in and out, mono, 44.1 kHz, MP3 at 64 kbit/s
  (about 10 KB, 1.2 s):

```bash
ffmpeg -i in.mp3 -af "atrim=0.34:1.52,asetpts=PTS-STARTPTS,highpass=f=90,volume=1.6dB,afade=t=in:d=0.01,afade=t=out:st=1.0:d=0.18" -ac 1 -ar 44100 -c:a libmp3lame -b:a 64k page-turn.mp3
```

In the sound, the leaf lifts during the first half and lands at about
0.8 s; book.js plays the whole sound for a turn and the second half (from
0.5 s) when a dragged page is let go and falls.
