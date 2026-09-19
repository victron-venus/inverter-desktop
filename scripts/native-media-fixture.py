#!/usr/bin/env python3
"""Disposable loopback-only delayed MJPEG fixtures for the native media harness."""
# CLI filenames use the repository's hyphenated script convention.
# pylint: disable=invalid-name

import argparse
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import subprocess
import tempfile
import time
from urllib.parse import urlsplit


def parse_args():
    """Validate the loopback port and explicitly selected disposable files."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--delay-ms", type=int, default=2000)
    parser.add_argument("--clip", type=Path)
    parser.add_argument("--ready-file", type=Path, required=True)
    args = parser.parse_args()
    if not 0 <= args.port <= 65535 or not 0 <= args.delay_ms <= 12000:
        parser.error("Invalid loopback port or delay (0..12000 ms)")
    if args.clip is not None and not args.clip.is_file():
        parser.error("--clip must name a readable disposable MP4 fixture")
    if not args.ready_file.is_absolute():
        parser.error("--ready-file must name a new absolute path")
    return args


def generate_frames(temporary):
    """Generate alternating, recognizable JPEG frames inside disposable storage."""
    frames = []
    for color in ["red", "blue"]:
        target = Path(temporary) / f"{color}.jpg"
        subprocess.run(
            [
                "ffmpeg", "-hide_banner", "-loglevel", "error", "-f", "lavfi",
                "-i", f"color=c={color}:s=640x360", "-frames:v", "1", str(target),
            ],
            check=True,
        )
        frames.append(target.read_bytes())
    return frames


def write_delayed_frame(stream, frame, delay_ms):
    """Withhold compressed pixels after delivering JPEG dimensions."""
    # Dimensions precede SOS. Metadata alone must not reveal the window.
    split = frame.index(b"\xff\xda") + 2
    stream.write(frame[:split])
    stream.flush()
    time.sleep(delay_ms / 1000)
    stream.write(frame[split:])


def fixture_handler(args, frames):
    """Bind the standard HTTP handler to this run's disposable fixture inputs."""

    class Fixture(BaseHTTPRequestHandler):
        """Serve disposable frames and deliberate startup failures on loopback."""

        protocol_version = "HTTP/1.1"

        def log_message(self, *_args):
            # Requests are intentionally quiet; stdout contains the ready record
            # and the native harness records its own timing and failure evidence.
            pass

        def do_GET(self):
            """Implement the standard handler API with explicit fixture routes."""
            path = urlsplit(self.path).path
            if path == "/prefix/clip.mp4" and args.clip is not None:
                self.send_clip()
                return
            if path not in {
                "/prefix/api/delayed", "/prefix/api/static",
                "/prefix/api/error", "/prefix/api/stall"
            }:
                self.send_error(404)
                return
            if path.endswith("/error"):
                time.sleep(args.delay_ms / 1000)
                self.send_error(404, "Intentional isolated camera fixture failure")
                return
            if path.endswith("/static"):
                self.send_static()
                return
            self.send_stream(path.endswith("/stall"))

        def send_clip(self):
            """Serve only the explicitly selected disposable MP4 fixture."""
            data = args.clip.read_bytes()
            self.send_response(200)
            self.send_header("Content-Type", "video/mp4")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)

        def send_static(self):
            """Expose JPEG metadata before delivering its decodable pixels."""
            frame = frames[0]
            self.send_response(200)
            self.send_header("Content-Type", "image/jpeg")
            self.send_header("Content-Length", str(len(frame)))
            self.send_header("Access-Control-Allow-Origin", "*")
            self.send_header("Cache-Control", "no-store")
            self.end_headers()
            write_delayed_frame(self.wfile, frame, args.delay_ms)

        def send_stream(self, stalled):
            """Keep multipart media open through the native preview lifetime."""
            self.send_response(200)
            self.send_header("Content-Type", "multipart/x-mixed-replace; boundary=frame")
            self.send_header("Cache-Control", "no-store")
            self.send_header("Access-Control-Allow-Origin", "*")
            self.end_headers()
            self.wfile.flush()
            # Headers arrive immediately; no image can decode during this wait.
            # The stream has no final boundary during the 15-second native lease.
            if stalled:
                time.sleep(40)
                # BaseHTTPRequestHandler owns this per-connection flag.
                self.close_connection = True  # pylint: disable=attribute-defined-outside-init
                return
            try:
                for index in range(80):
                    frame = frames[index % len(frames)]
                    self.wfile.write(
                        b"--frame\r\nContent-Type: image/jpeg\r\nContent-Length: "
                        + str(len(frame)).encode("ascii") + b"\r\n\r\n"
                    )
                    if index == 0:
                        write_delayed_frame(self.wfile, frame, args.delay_ms)
                    else:
                        self.wfile.write(frame)
                    self.wfile.write(b"\r\n")
                    self.wfile.flush()
                    time.sleep(0.5)
            except (BrokenPipeError, ConnectionResetError):
                # The native preview deliberately disconnects before EOF.
                pass
            self.close_connection = True  # pylint: disable=attribute-defined-outside-init

    return Fixture


def write_ready(args, port):
    """Publish exact fixture URLs without overwriting an earlier run's receipt."""
    base = f"http://127.0.0.1:{port}/prefix"
    ready = {
        "delayed": f"{base}/api/delayed?fps=2&height=360",
        "static": f"{base}/api/static?fps=2&height=360",
        "error": f"{base}/api/error?fps=2&height=360",
        "stall": f"{base}/api/stall?fps=2&height=360",
        "clip": f"{base}/clip.mp4" if args.clip else None,
        "delay_ms": args.delay_ms,
    }
    with args.ready_file.open("x", encoding="utf-8") as output:
        json.dump(ready, output, indent=2)
        output.write("\n")
    print(json.dumps(ready), flush=True)


def main():
    """Run an isolated loopback fixture and clean up its temporary media on exit."""
    args = parse_args()
    with tempfile.TemporaryDirectory(prefix="native-media-fixture-") as temporary:
        frames = generate_frames(temporary)
        server = ThreadingHTTPServer(("127.0.0.1", args.port), fixture_handler(args, frames))
        server.daemon_threads = True
        write_ready(args, server.server_port)
        try:
            # S5332: binding is hardcoded to loopback; payloads are generated frames
            # or an explicit disposable test MP4, with no credentials. HTTP is the
            # native harness contract, not a production network server.
            server.serve_forever()  # NOSONAR - S5332: isolated loopback-only HTTP fixture.
        except KeyboardInterrupt:
            pass
        finally:
            server.server_close()


if __name__ == "__main__":
    main()
