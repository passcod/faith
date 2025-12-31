#!/bin/sh

file="${1:-network-trace.pcap}"
keylog="$(realpath "${file%.pcap}.keylog")"

if [[ -f "$keylog" ]]; then
	exec wireshark -o "tls.keylog_file:$keylog" "$file"
else
	exec wireshark "$file"
fi
