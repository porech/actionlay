# Usage: source scripts/env.sh
ACTIONLAY_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]:-$0}")/.." && pwd)"
export FFMPEG_DIR="$ACTIONLAY_ROOT/third_party/ffmpeg/$(rustc -vV | sed -n 's/^host: //p')"
