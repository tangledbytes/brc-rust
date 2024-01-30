#!/bin/bash

# Install Java 21
echo "Installing Java 21"
wget https://download.oracle.com/java/21/latest/jdk-21_linux-x64_bin.deb
apt install ./jdk-21_linux-x64_bin.deb

# Create ramdisk
mkdir /media/ramdisk
mount -t ramfs ramfs /media/ramdisk

pushd /media/ramdisk || exit

# Clone 1BRC repository
git clone https://github.com/gunnarmorling/1brc.git
pushd 1brc || exit
./mvnw clean verify
./create_measurements.sh 1000000000
popd || exit

# Clone the brc-rust repository
git clone https://github.com/tangledbytes/brc-rust.git
pushd brc-rust || exit

# Install rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
