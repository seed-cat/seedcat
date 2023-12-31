# Tests for when working on the hashcat module

set -e
source ./scripts/configure_hashcat.sh

if [ -z $1 ]; then
  echo "Usage ./scripts/test_hashcat.sh <test|run|bench> <#|all> [clean]"
  echo "  'test' executes one hash (good for debugging)"
  echo "  'run' executes multiple hashes"
  echo "  'bench' executes for a long time without finding a solution"
  echo "  'build' executes the make command and terminates"
  exit 1
fi

rm -f modules/module_28510.so
rm -f kernels/*
rm -f hashcat.potfile

if [[ "$3" = "clean" ]]; then
  make clean
fi
make

if [[ "$1" = "build" ]]; then
  exit 0
fi

TEST1="XPUB:m/:coral,dice,harvest:xpub661MyMwAqRbcFaizXLqdLrqBkUJo4JyyXYNucU2hWQBDfhmCd3TL7USdpjhUddedvEiSo31BRg9QB4a5PNKcuQRWT6DA2YveGA2tzsqZQwg"
PASS1="hashca?4"
EXPECTED1="xpub661MyMwAqRbcFaizXLqdLrqBkUJo4JyyXYNucU2hWQBDfhmCd3TL7USdpjhUddedvEiSo31BRg9QB4a5PNKcuQRWT6DA2YveGA2tzsqZQwg:hashcat"

TEST2="P2PKH:m/0/0,m/44h/0h/0h/0/0:balcony,catalog,winner,letter,alley,this:1NSHnSJpJPWXgMAXk88ktQPTFyniPeUaod"
PASS2="hashca?4"
EXPECTED2="1NSHnSJpJPWXgMAXk88ktQPTFyniPeUaod:hashcat"

TEST3="P2SH-P2WPKH:m/49h/0h/0h/0/1:cage,keep,stone,swarm,open,race,toward,state,subway,dutch,extra,short,purpose,interest,enough,idle,found,guilt,will,salt,mixed,boil,heavy,thing:361yU4TkuRSLTdTkfEUbWGfTJgJjFDZUvG"
PASS3="hashca?4"
EXPECTED3="361yU4TkuRSLTdTkfEUbWGfTJgJjFDZUvG:hashcat"

TEST4="P2WPKH:m/84h/0h/0h/0/2:donate,dolphin,bachelor,excess,stuff,flower,spread,crazy,scorpion,zoo,skull,lottery:bc1q490ra0dcf4l58jzt2445akrxpj6aftkfdvs8n7"
PASS4="hashca?4"
EXPECTED4="bc1q490ra0dcf4l58jzt2445akrxpj6aftkfdvs8n7:hashcat"

# security sugar abandon diamond abandon orient zoo example crane fruit senior decade
# '=' means include in the result
TEST5="P2WPKH:m/84h/0h/0h/0/0:=security,sugar,?,=diamond,?,orient,?,example,crane,fruit,senior,?:bc1q6dlx8mxcxm3qterx35cul7z76v975tf2vq06yr"
PASS5="5656?1?2?3hashcat"
EXPECTED5="bc1q6dlx8mxcxm3qterx35cul7z76v975tf2vq06yr:security,abandon,diamond,abandon,zoo,decade,hashcat"

# can also use numeric indexes (faster and more compact to parse)
TEST6="P2WPKH:m/84h/0h/0h/0/0:1558,1734,0,489,0,1252,2047,627,402,750,1565,?:bc1q6dlx8mxcxm3qterx35cul7z76v975tf2vq06yr"
PASS6="?3hashcat"
EXPECTED6="bc1q6dlx8mxcxm3qterx35cul7z76v975tf2vq06yr:decade,hashcat"

# m/84'/0'/4'/0/5
TEST7="P2WPKH:m/84h/0h/?5h/0/?5:paper,warrior,title,join,assume,trumpet,setup,angle,helmet,salmon,save,love:bc1qhatcz9ljzuucd6en9sr3p9mlt7t78654h9hqf6"
PASS7="hashca?4"
EXPECTED7="bc1qhatcz9ljzuucd6en9sr3p9mlt7t78654h9hqf6:hashcat"

TEST8="P2PKH:m/0/0,m/44h/0h/0h/0/0:security,sugar,abandon,diamond,abandon,orient,?,example,crane,fruit,senior,?:1HXGvcN88JpBPAFhd1CmjMKcbbM8H2a9TP"
PASS8="?1?2?3"
EXPECTED8="1HXGvcN88JpBPAFhd1CmjMKcbbM8H2a9TP:zoo,decade,"

TOTAL=8

for i in $(seq 1 $TOTAL);
do
  if [[ "$2" = "$i" ]] || [[ "$2" = "all" ]]; then
    echo "Running $1 #$i"
    TEST="TEST$i"
    PASS="PASS$i"
    EXPECTED="EXPECTED$i"
    if [ $1 = "test" ]; then
      ./hashcat -m 28510 -a 3 --self-test-disable --force -n 1 -u 1 -T 1 -1 T -2 u -3 S -4 t "${!TEST}" "${!PASS}"
    elif [ $1 = "run" ]; then
      ./hashcat -m 28510 -a 3  --self-test-disable -1 charsets/bin/5bit.hcchr -2 charsets/bin/6bit.hcchr -3 charsets/bin/7bit.hcchr -4 ?l "${!TEST}" "${!PASS}"
    elif [ $1 = "bench" ]; then
      ./hashcat -m 28510 --self-test-disable -a 3 -1 charsets/bin/5bit.hcchr -2 charsets/bin/6bit.hcchr -3 charsets/bin/7bit.hcchr -4 ?l --status "${!TEST}" "?1?2?1?2?1?2?3?l"
    fi

    # Validate results
    RESULT=$( tail -n 1 hashcat.potfile )
    if [[ $RESULT = "${!EXPECTED}" ]]; then
      echo -e "\n========== Test $i Passed =========="
    else
      echo -e "\n========== Test $i Failed =========="
      echo "RESULT: $RESULT"
      echo "EXPECTED: ${!EXPECTED}"
      exit 1
    fi
  fi
done

echo -e "\n========== All Results =========="
cat hashcat.potfile
