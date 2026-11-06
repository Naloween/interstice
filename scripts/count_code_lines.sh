cd "$(dirname "$0")/.."
cloc --include-ext=rs --exclude-dir=target,vendor,third_party,deps,.git .
