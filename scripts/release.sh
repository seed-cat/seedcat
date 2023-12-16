# Builds a new seedcat_*.zip release
#
# Releases are built in a Ubuntu 18.04 Desktop image https://releases.ubuntu.com/18.04/
# Ensures >2018-02-01 GLIBC compatibility and support until April 2028
# You can run the image in VirtualBox or similar VM software
# Setup your Ubuntu VM for builds with the following commands:
#
# su
# sudo apt update
# sudo apt install gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 git curl -y
# git clone https://github.com/seed-cat/seedcat
# git clone https://github.com/seed-cat/win-iconv
# cd win-iconv/
# patch < ../hashcat/tools/win-iconv-64.diff
# sudo make install
# cd ..
# curl https://sh.rustup.rs -sSf | sh
# rustup target add x86_64-pc-windows-gnu

SC="seedcat"
HC="$SC/hashcat"

set -e
source ./scripts/configure_hashcat.sh

make clean
make binaries
cd ..
cargo build --release --target x86_64-pc-windows-gnu
cargo build --release --target x86_64-unknown-linux-gnu

cd ..
rm -f "$PROJECT_NAME.zip"
cp "./$SC/target/x86_64-pc-windows-gnu/release/seedcat.exe" $SC
cp "./$SC/target/x86_64-unknown-linux-gnu/release/seedcat" $SC

zip -r "$PROJECT_NAME.zip" . -i $SC/docs/*  $SC/scripts/* $SC/dicts/* $HC/hashcat.exe $HC/hashcat.bin $HC/hashcat.hcstat2 \
$HC/modules/*.so $HC/modules/*.dll $HC/OpenCL/*.cl $HC/OpenCL/*.h $HC/charsets/* $HC/charsets/*/*

zip -r "$PROJECT_NAME.zip" -m $SC/seedcat.exe $SC/seedcat
mv "$PROJECT_NAME.zip" $SC