"""Minecraft-wire test fixture. Not a Minecraft implementation or a production server."""
import argparse
import json
import socket
import threading
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--port", type=int, required=True)
parser.add_argument("--directory")
args = parser.parse_args()
log_path = Path(args.directory) / 'logs/latest.log' if args.directory else None
if log_path:
    log_path.parent.mkdir(parents=True, exist_ok=True)


def emit(text, file_only=False):
    if log_path:
        with log_path.open('a', encoding='utf-8') as file:
            file.write(text + '\n')
    if not file_only:
        print(text, flush=True)
listener = socket.socket()
listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
listener.bind(("127.0.0.1", args.port))
listener.listen()


def varint(value):
    result = bytearray()
    while True:
        byte = value & 127
        value >>= 7
        result.append(byte | (128 if value else 0))
        if not value:
            return bytes(result)


def read_varint(connection):
    value = 0
    for shift in range(0, 35, 7):
        byte = connection.recv(1)
        if not byte:
            raise EOFError()
        value |= (byte[0] & 127) << shift
        if byte[0] < 128:
            return value
    raise ValueError("Invalid VarInt")


def serve_status():
    while True:
        connection, _ = listener.accept()
        try:
            connection.settimeout(3)
            for _ in range(2):
                remaining = read_varint(connection)
                while remaining:
                    chunk = connection.recv(remaining)
                    if not chunk:
                        raise EOFError()
                    remaining -= len(chunk)
            payload = json.dumps({
                "version": {"name": "Paper fixture 1.21.4", "protocol": 767},
                "description": {"text": "Browser-tested world\nReady to build."},
                "players": {"online": 1, "max": 20, "sample": [
                    {"name": "Alex", "id": "ec561538-f3fd-461d-aff5-086b22154bce"}
                ]},
            }).encode()
            body = b"\0" + varint(len(payload)) + payload
            connection.sendall(varint(len(body)) + body)
        except (OSError, EOFError, ValueError):
            pass
        finally:
            connection.close()


threading.Thread(target=serve_status, daemon=True).start()
emit("[Server thread/INFO]: Done (0.1s)! Fixture ready.")
emit("[Server thread/INFO]: File-only connection: Alex logged in", file_only=True)
emit("[Server thread/INFO]: There are 0 of a max of 20 players online: ", file_only=True)
try:
    while True:
        command = input().strip()
        if command == "stop":
            emit("[Server thread/INFO]: Alex left the game")
            emit("[Server thread/INFO]: World saved. Fixture stopped.")
            break
        if command.startswith("help"):
            emit("[Server thread/INFO]: /help: Show commands")
            emit("[Server thread/INFO]: /list: List players")
            emit("[Server thread/INFO]: /say: Send a message")
            emit("[Server thread/INFO]: /stop: Stop server")
        elif command == "list":
            emit("[Server thread/INFO]: There are 1 of a max of 20 players online: Alex")
        else:
            emit("[Server thread/INFO]: Executed: " + command)
except EOFError:
    pass
finally:
    listener.close()
