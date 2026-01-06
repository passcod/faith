#!/bin/bash

set -e

PCAP_FILE="${1:-network-trace.pcap}"
IMAGE="${PODMAN_IMAGE:-working}"
COMMAND="${@:2}"

KEYLOG_FILE="${PCAP_FILE%.pcap}.keylog"
TEMP_PCAP="/tmp/netrace_$$.pcap"
TEMP_KEYLOG="/tmp/netrace_$$.keylog"

cleanup() {
  if [ -n "$CONTAINER_ID" ]; then
    sudo podman kill "$CONTAINER_ID" 2>&1 > /dev/null || true
    sudo podman rm "$CONTAINER_ID" 2>&1 > /dev/null || true
  fi
}

trap cleanup EXIT

# Check if image exists
if ! sudo podman image inspect "$IMAGE" > /dev/null; then
  echo "Error: Image '$IMAGE' not found." >&2
  echo "Please build the image with: sudo podman build -t $IMAGE ." >&2
  exit 1
fi

CONTAINER_ID=$(sudo podman create -it --privileged --cap-add=NET_ADMIN --network podman-ipv6 -e TARGET="$TARGET" -e HITS="$HITS" -e HTTP3="$HTTP3" -e SSLKEYLOGFILE="/tmp/sslkeylog.txt" --entrypoint /bin/sh "$IMAGE" -c "while true; do sleep 1; done" 2>/dev/null)

sudo podman start "$CONTAINER_ID" > /dev/null 2>&1

# We need tcpdump instead of tshark here so we can use --immediate-mode
sudo podman exec "$CONTAINER_ID" tcpdump --immediate-mode -KU -i eth0 -w /tmp/tcpdump.pcap &

TCPDUMP_PID=$!

sudo podman exec "$CONTAINER_ID" $COMMAND

if [ -n "$TCPDUMP_PID" ]; then
  kill $TCPDUMP_PID 2>/dev/null || true
  wait $TCPDUMP_PID 2>/dev/null || true
fi

TSHARK_FILTER="${TSHARK_FILTER:-tcp or dns or quic}"
if [ -n "$TSHARK_FILTER" ]; then
  echo "Filtering to '$TSHARK_FILTER'"
  sudo podman exec "$CONTAINER_ID" tshark -r /tmp/tcpdump.pcap -w /tmp/filtered.pcap -Y "$TSHARK_FILTER" > /dev/null 2>&1
  sudo podman cp "$CONTAINER_ID":/tmp/filtered.pcap "$PCAP_FILE"
else
  sudo podman cp "$CONTAINER_ID":/tmp/tcpdump.pcap "$PCAP_FILE"
fi

sudo chown $(id -u):$(id -g) "$PCAP_FILE"

# Copy keylog file if it exists
if sudo podman exec "$CONTAINER_ID" test -f /tmp/sslkeylog.txt 2>/dev/null; then
  sudo podman cp "$CONTAINER_ID":/tmp/sslkeylog.txt "$KEYLOG_FILE"
  sudo chown $(id -u):$(id -g) "$KEYLOG_FILE"
fi

tcpdump --count -r "$PCAP_FILE"
