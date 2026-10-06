#!/usr/bin/env bash
# Public Garmin examples for local validation; never committed or redistributed.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
base="https://raw.githubusercontent.com/garmin/fit-javascript-sdk/f73a9fbd509bc1d5c63047d70cd68c0362b9aeed"
mkdir -p "$root/samples/external"
for name in Activity WithGearChangeData; do
  curl --fail --location --retry 2 "$base/test/data/$name.fit" -o "$root/samples/external/$name.fit"
done
curl --fail --location "$base/LICENSE.txt" -o "$root/samples/external/Garmin-LICENSE.txt"
cd "$root/samples/external"
shasum -a 256 -c <<'SUMS'
949a238e1bb75c3684479785f76fa9a16888bb394518844248f488171d591387  Activity.fit
7220d6a86e0bafcb1884070081757a2a65caa83d46afccc113ebbe17df60fa79  WithGearChangeData.fit
SUMS
