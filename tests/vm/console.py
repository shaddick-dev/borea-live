"""Drive a VM's serial console: send lines, wait for text. Used by the install test."""
import socket, time, re, uuid

class Console:
    def __init__(self, path, log):
        for _ in range(100):
            try:
                self.s = socket.socket(socket.AF_UNIX); self.s.connect(path); break
            except OSError:
                self.s.close()
                time.sleep(0.2)
        else:
            raise ConnectionError(f'Could not connect to serial console {path}')
        self.s.settimeout(0.5)
        self.buf = ''
        self.log = open(log, 'a')

    def read(self):
        try:
            data = self.s.recv(65536).decode(errors='replace')
        except socket.timeout:
            return
        if not data:
            raise ConnectionError('VM serial console closed')
        data = re.sub(r'\x1b\][^\\]*\\', '', data)
        self.buf += data
        self.log.write(data); self.log.flush()

    def expect(self, pattern, timeout):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            m = re.search(pattern, self.buf)
            if m:
                self.buf = self.buf[m.end():]
                return m
            self.read()
        raise TimeoutError(f'waited {timeout}s for {pattern!r}; last output:\n{self.buf[-1500:]}')

    def send(self, line):
        self.s.sendall((line + '\n').encode())

    def run(self, cmd, timeout=60):
        """Run a shell command and return its status, including pipeline failures."""
        tag = f'__done{uuid.uuid4().hex}'
        self.buf = ''
        self.send(f'set -o pipefail; {cmd}; echo {tag}=$?')
        m = self.expect(tag + r'=(\d+)', timeout)
        return int(m.group(1))

    def close(self):
        self.s.close()
        self.log.close()
