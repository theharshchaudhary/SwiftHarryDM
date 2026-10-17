"""Single-instance lock: two app instances must never share one state database."""
import msvcrt
import os


class InstanceLock:
    def __init__(self, path):
        self.path = str(path)
        self._fd = None

    def acquire(self) -> bool:
        fd = os.open(self.path, os.O_RDWR | os.O_CREAT)
        try:
            msvcrt.locking(fd, msvcrt.LK_NBLCK, 1)
        except OSError:
            os.close(fd)
            return False
        self._fd = fd
        return True

    def release(self):
        if self._fd is not None:
            try:
                os.lseek(self._fd, 0, os.SEEK_SET)
                msvcrt.locking(self._fd, msvcrt.LK_UNLCK, 1)
            except OSError:
                pass
            os.close(self._fd)
            self._fd = None
