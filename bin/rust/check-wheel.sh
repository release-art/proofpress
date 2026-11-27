#!/bin/bash
# bin/rust/check-wheel.sh WHEEL [VERSION]   fail if the wheel is not one abi3 wheel for every CPython from 3.11, if its
# extension links a shared library outside the operating system, or (with VERSION) if the wheel's version is not VERSION. The release job runs it on every wheel it builds.
set -euo pipefail
wheel="$1"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
unzip -q "$wheel" -d "$work"
if [[ -n "${2:-}" ]]; then
  got=$(sed -n 's/^Version: //p' "$work"/*.dist-info/METADATA)
  [[ "$got" == "$2" ]] || { echo "wheel version $got, expected $2" >&2; exit 1; }
fi
grep -q '^Tag: cp311-abi3-' "$work"/*.dist-info/WHEEL || { echo "$(basename "$wheel") is not a cp311-abi3 wheel" >&2; exit 1; }
status=0
for lib in $(find "$work" -name '_core*.so' -o -name '_core*.pyd'); do
  case "$(uname -s)" in
    # Windows: the DLLs the extension imports, read with pefile (pip install pefile). System DLLs, the C runtime and
    # Python's own are part of any Windows Python installation.
    MINGW* | MSYS* | CYGWIN*) deps=$(python -c "import pefile, sys; pe = pefile.PE(sys.argv[1]); print('\n'.join(e.dll.decode() for e in pe.DIRECTORY_ENTRY_IMPORT))" "$lib" |
         grep -v -i -E '^(kernel32|ntdll|advapi32|bcrypt|bcryptprimitives|ws2_32|userenv|shell32|ole32|oleaut32|synchronization|api-ms-win-[a-z0-9-]+|vcruntime140(_1)?|ucrtbase|python3[0-9]*)\.dll$' || true) ;;
    Darwin) deps=$(otool -L "$lib" | tail -n +2 | awk '{print $1}' | grep -v -E '^(/usr/lib/|/System/|@rpath/proof_pdf)' || true) ;;
    # glibc and the compiler runtime are the OS's; manylinux allows exactly these.
    *) deps=$(readelf -d "$lib" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p' |
         grep -v -E '^(libc|libm|libdl|libpthread|librt|libgcc_s|libutil|ld-linux[^ ]*)\.so' || true) ;;
  esac
  if [[ -n "$deps" ]]; then
    echo "$(basename "$lib") links libraries outside the OS:" >&2
    echo "$deps" >&2
    status=1
  fi
done
[[ $status == 0 ]] && echo "$(basename "$wheel"): only OS libraries"
exit $status
