"""Cuts record.mjs's page videos into demo.mp4 and demo.gif, ending on the slide.

Usage: python3 assemble.py   (reads and writes $OUT, default /tmp/quomodo-demo-rec)
"""
import json, os, shutil, subprocess

OUT = os.environ.get("OUT", "/tmp/quomodo-demo-rec")
FF = os.environ.get("FFMPEG") or shutil.which("ffmpeg")
d = json.load(open(f"{OUT}/marks.json"))
m = {k: v["t"] for k, v in d["marks"].items()}
v = d["videos"]
# (video, start, end, speed)
segs = [
    (v["s1"], m["s1-start"], m["s1-end"], 1.15),
    (v["ui"], m["push"], m["deployed"], 3.0),
    (v["ui"], m["deployed"], m["dialog"], 1.0),
    (v["ui"], m["dialog"], m["dialog"] + 3.6, 1.0),
    (v["s3"], m["s3-start"], m["s3-start"] + 4.2, 1.0),
    (v["servers"], m["s4-start"], m["s4-end"], 1.0),
    (v["stacks"], m["s5-start"], m["s5-end"], 1.0),
]
inputs, parts = [], []
for i, (f, a, b, sp) in enumerate(segs):
    inputs += ["-i", f]
    parts.append(f"[{i}:v]trim=start={a:.3f}:end={b:.3f},setpts=(PTS-STARTPTS)/{sp},fps=25,format=yuv420p[s{i}]")
main = sum((b - a) / sp for _, a, b, sp in segs)
SLIDE, FADE = 3.5, 0.5
n = len(segs)
inputs += ["-loop", "1", "-t", str(SLIDE + FADE), "-i", f"{OUT}/slide.png"]
parts.append(f"[{n}:v]fps=25,format=yuv420p,settb=AVTB[sl]")
fc = (";".join(parts) + ";" + "".join(f"[s{i}]" for i in range(n)) + f"concat=n={n}:v=1:a=0,settb=AVTB[main];"
      f"[main][sl]xfade=transition=fade:duration={FADE}:offset={main - FADE:.3f}[out]")
subprocess.run([FF, "-loglevel", "error", "-y", *inputs, "-filter_complex", fc, "-map", "[out]",
                "-c:v", "libx264", "-crf", "20", "-preset", "slow", "-movflags", "+faststart", f"{OUT}/demo.mp4"], check=True)
GIF_W, GIF_FPS = int(os.environ.get("GIF_W", 960)), int(os.environ.get("GIF_FPS", 10))
subprocess.run([FF, "-loglevel", "error", "-y", "-i", f"{OUT}/demo.mp4", "-filter_complex",
    f"fps={GIF_FPS},scale={GIF_W}:-1:flags=lanczos,split[a][b];[a]palettegen=max_colors=256:stats_mode=full[p];[b][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle",
    f"{OUT}/demo.gif"], check=True)
