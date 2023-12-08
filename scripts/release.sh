# For releasing a new executable
#
# Releases are built in a Ubunutu 18.04 image https://releases.ubuntu.com/18.04/
# Ensures >2018-02-01 GLIBC compatibility and support until April 2028
# objdump -T hashcat.bin | grep GLIBC | sed 's/.*GLIBC_\([.0-9]*\).*/\1/g' | sort -Vu
# Machine setup is as follows for cross-compiling:
#
# sudo apt update
# sudo apt install gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 make git curl
# git clone https://github.com/hashcat/hashcat
# git clone https://github.com/win-iconv/win-iconv
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