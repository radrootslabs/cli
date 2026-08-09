#!/bin/sh
set -eu

if [ "$#" -gt 1 ]; then
  echo "usage: $0 [repository-root]" >&2
  exit 64
fi

if [ "$#" -eq 1 ]; then
  repository_root=$1
else
  CDPATH=''
  export CDPATH
  repository_root=$(cd -- "$(dirname -- "$0")/.." && pwd -P)
fi

if [ ! -d "$repository_root" ]; then
  echo "repository root is not a directory: $repository_root" >&2
  exit 64
fi

status=0
for forbidden_root in docs .github .act; do
  candidate=$repository_root/$forbidden_root
  if [ -e "$candidate" ] || [ -L "$candidate" ]; then
    echo "forbidden capsule root exists: $forbidden_root" >&2
    status=1
  fi
done

exit "$status"
