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

HC="hashcat"

set -e
source ./scripts/configure_hashcat.sh

make clean
make binaries
cd ..

cargo build --release --target x86_64-pc-windows-gnu
cargo build --release --target x86_64-unknown-linux-gnu

rm -f "$PROJECT_NAME.zip"
zip -r "$PROJECT_NAME.zip" . -i dicts/* $HC/hashcat.exe -i $HC/hashcat.bin -i $HC/hashcat.hcstat2 $HC/modules/*.so \
  $HC/modules/*.dll $HC/OpenCL/*.cl $HC/OpenCL/*.h $HC/charsets/* $HC/charsets/*/*
zip -ju "$PROJECT_NAME.zip" "./target/x86_64-pc-windows-gnu/release/$PROJECT_NAME.exe"
zip -ju "$PROJECT_NAME.zip" "./target/x86_64-unknown-linux-gnu/release/$PROJECT_NAME"