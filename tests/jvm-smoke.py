"""Optional real-JVM integration check. Run after cargo build with a Java 21 JDK.
VOID_JDK_BIN can select the JDK's bin directory; no Minecraft server is needed.
"""
import http.cookiejar
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import tempfile
import time
import urllib.request

root = Path(__file__).resolve().parent.parent
javac = Path(os.environ.get('VOID_JDK_BIN', Path(shutil.which('javac') or '/missing').parent)) / ('javac.exe' if os.name == 'nt' else 'javac')
java = javac.parent / ('java.exe' if os.name == 'nt' else 'java')
assert javac.is_file(), 'A Java 21 JDK is required (set VOID_JDK_BIN).'
binary = os.environ.get('VOID_DASH_BINARY', str(root / 'target/debug/void-dash'))
with tempfile.TemporaryDirectory(prefix='void-real-jvm-') as temporary:
    directory = Path(temporary)
    source = directory / 'MemoryFixture.java'
    source.write_text('''
import java.io.*;
public class MemoryFixture {
 static final byte[] retained = new byte[24 * 1024 * 1024];
 public static void main(String[] args) throws Exception {
  retained[0]=42; System.out.println("JVM fixture ready");
  BufferedReader reader=new BufferedReader(new InputStreamReader(System.in));
  for(String line; (line=reader.readLine())!=null;) { if(line.equals("stop")) break; }
 }
}''')
    subprocess.run([str(javac), '-J-Djava.home=' + str(javac.parent.parent), '--release', '21', '-d', temporary, str(source)], check=True)
    with socket.socket() as reservation:
        reservation.bind(('127.0.0.1', 0))
        port = reservation.getsockname()[1]
    environment = dict(os.environ, JAVA_HOME=str(javac.parent.parent))
    environment['PATH'] = str(javac.parent) + os.pathsep + environment.get('PATH', '')
    application = subprocess.Popen([binary, '--no-open', '--port', str(port), '--config', str(directory/'config.json')], env=environment, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    client = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))
    base = f'http://127.0.0.1:{port}'
    server_id = None
    def request(path, method='GET', body=None):
        encoded = json.dumps(body).encode() if body is not None else None
        with client.open(urllib.request.Request(base+path, data=encoded, method=method, headers={'Content-Type':'application/json'}), timeout=35) as response:
            return json.load(response)
    try:
        for _ in range(100):
            try:
                client.open(base, timeout=1).close()
                break
            except OSError:
                time.sleep(.1)
        server = request('/api/servers','POST',{'name':'Real JVM heap fixture','host':'127.0.0.1','port':9,'mode':'local','directory':temporary,'executable':str(java),'args':['-Xms32m','-Xmx96m','-cp',temporary,'MemoryFixture']})
        server_id = server['id']
        request(f'/api/servers/{server_id}/action/start','POST')
        sample = None
        for _ in range(60):
            sample = request(f'/api/servers/{server_id}/state')
            if (sample.get('resources') or {}).get('heap_used') is not None:
                break
            time.sleep(.5)
        assert sample and sample['managed'], sample
        resources = sample['resources']
        assert resources['heap_max'] == 96*1024**2, resources
        assert 24*1024**2 <= resources['heap_used'] <= resources['heap_max'], resources
        assert resources['memory_total'] > resources['heap_max'], resources
        assert sample['uptime_seconds'] is not None, sample
        print('Real JVM verified through Rust API: used heap within its actual 96 MiB maximum, separate host memory, and server uptime.')
    finally:
        if server_id:
            try:
                request(f'/api/servers/{server_id}/action/stop','POST')
            except OSError:
                pass
        application.terminate()
        application.wait(timeout=10)
