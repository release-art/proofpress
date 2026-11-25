#!/bin/bash
# bin/rust/check-wheel.sh WHEEL [VERSION]   fail if the wheel's extension links a shared library outside the operating
# system, or (with VERSION) if the wheel's version is not VERSION. The release job runs it on every wheel it builds.
set -euo pipefail
wheel="$1"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
unzip -q "$wheel" -d "$work"
if [[ -n "${2:-}" ]]; then
  got=$(sed -n 's/^Version: //p' "$work"/*.dist-info/METADATA)
  [[ "$got" == "$2" ]] || { echo "wheel version $got, expected $2" >&2; exit 1; }
fi
status=0
for lib in $(find "$work" -name '_core*.so'); do
  case "$(uname -s)" in
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
