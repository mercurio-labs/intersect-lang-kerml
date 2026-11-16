# Regenerates python-deflate.zip and python-streamed.zip (independent ZIP
# writer fixtures for zip_tests.rs): python -I make_zip_fixtures.py <this dir>
import io, sys, zipfile
out = sys.argv[1]
files = [(".project.json", '{"name":"P","version":"1.0.0"}'),
         (".meta.json", '{"index":{"P":"pkg/P.sysml"},"created":"2026-01-01T00:00:00Z"}'),
         ("pkg/P.sysml", "package P;\n")]
def fixed(name):
    info = zipfile.ZipInfo(name, date_time=(2026, 1, 1, 0, 0, 0))
    info.compress_type = zipfile.ZIP_DEFLATED
    return info
with zipfile.ZipFile(out + "/python-deflate.zip", "w") as z:
    for name, text in files:
        z.writestr(fixed(name), text)
class Unseekable(io.RawIOBase):
    def __init__(self): self.buf = bytearray()
    def writable(self): return True
    def write(self, b): self.buf += b; return len(b)
    def seekable(self): return False
    def tell(self): raise OSError("unseekable")
sink = Unseekable()
with zipfile.ZipFile(sink, "w", compression=zipfile.ZIP_DEFLATED) as z:
    for name, text in files:
        with z.open(fixed(name), "w") as f:
            f.write(text.encode())
open(out + "/python-streamed.zip", "wb").write(bytes(sink.buf))
