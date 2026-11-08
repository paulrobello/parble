"""Static preview of target/web-dist for the Playwright suite (e2e/playwright.config.mjs).

`python3 -m http.server` listens with socketserver's default backlog of 5 and answers HTTP/1.0, so
every fetch is a new connection. The kit2000 scenario starts about 30 texture fetches at once; on
macOS a SYN that finds the accept queue full is answered with RST, so Chromium reports
net::ERR_CONNECTION_RESET and the asset fails to load without the request ever reaching the server.
This server is the same handler with a backlog sized for the page's burst.

Usage: python3 static-server.py <port> <directory>
"""

import functools
import http.server
import sys


class Server(http.server.ThreadingHTTPServer):
    request_queue_size = 128
    daemon_threads = True


def main() -> None:
    port, directory = int(sys.argv[1]), sys.argv[2]
    handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=directory)
    with Server(("127.0.0.1", port), handler) as httpd:
        print(f"serving {directory} on http://127.0.0.1:{port}/", flush=True)
        httpd.serve_forever()


if __name__ == "__main__":
    main()
