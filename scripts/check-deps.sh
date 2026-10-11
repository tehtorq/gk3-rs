#!/bin/sh
# Fails if the release binary links any shared library beyond the C runtime.
# Graphics, display and audio libraries must be loaded with dlopen at run time,
# never linked, so that a missing one is a message rather than a failed launch.
set -eu
bin="${1:-target/release/gk3}"
allowed='linux-vdso|libc\.so|libm\.so|libgcc_s\.so|ld-linux'

echo "Shared libraries linked by $bin:"
ldd "$bin" | sed 's/^/  /'

extra=$(ldd "$bin" | grep -Ev "$allowed" || true)
if [ -n "$extra" ]; then
    echo "error: unexpected link-time dependencies:" >&2
    echo "$extra" >&2
    exit 1
fi

glibc=$(objdump -T "$bin" | grep -o 'GLIBC_[0-9.]*' | sort -uV | tail -1)
echo "Newest glibc symbol version required: $glibc"
echo "OK"
