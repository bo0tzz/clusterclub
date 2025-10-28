#!/usr/bin/env python3
"""Simple HTTP backend server for testing"""

import sys
from http.server import HTTPServer, BaseHTTPRequestHandler

class TestHandler(BaseHTTPRequestHandler):
    def do_GET(self):
        port = self.server.server_port
        self.send_response(200)
        self.send_header('Content-type', 'text/plain')
        self.end_headers()
        self.wfile.write(f"Response from backend on port {port}\n".encode())

    def log_message(self, format, *args):
        sys.stdout.write(f"[Backend {self.server.server_port}] {format % args}\n")

if __name__ == '__main__':
    if len(sys.argv) != 2:
        print(f"Usage: {sys.argv[0]} <port>")
        sys.exit(1)

    port = int(sys.argv[1])
    server = HTTPServer(('127.0.0.1', port), TestHandler)
    print(f"Backend server listening on 127.0.0.1:{port}")
    server.serve_forever()
