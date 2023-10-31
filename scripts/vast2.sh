# Demo of distributed cracking with multiple vast.ai instances
#
# Finds the last missing 4 seed words from the following wallet:
# army excuse hero wolf disease liberty moral diagram treat stove message job

# The IP of the server that will run the brain
BRAIN_IP=199.195.151.121
# The port you setup the docker container with e.g. "-p 9000:9000"
BRAIN_INTERNAL_PORT=9000
# Exposed docker port e.g. "199.195.151.121:40386 -> 9000/tcp"
BRAIN_EXTERNAL_PORT=40386
# Password doesn't matter so long as it's the same for the server and client
BRAIN_PASSWORD=ef5f0654c472dbd3

[ -z $1 ] && echo "You must supply the IP:PORT of the instance" && exit 1
SSH_IP=`echo $1 | cut -d':' -f1`
SSH_PORT=`echo $1 | cut -d':' -f2`

echo "apt install -y p7zip-full" > run.sh
echo "7z x -r bitcoin-recovery-tool.7z" >> run.sh
echo "cd bitcoin-recovery-tool" >> run.sh

if [ $SSH_IP = $BRAIN_IP ]; then
  echo "Running for brain SERVER $SSH_IP port $SSH_PORT"
  read -p "Press enter to continue"
  #echo "./hashcat.bin -m 28510 --brain-server --brain-password $BRAIN_PASSWORD --brain-port $BRAIN_INTERNAL_PORT -w 4 --status --self-test-disable -a 3 -1 charsets/bin/5bit.hcchr -2 charsets/bin/6bit.hcchr -3 charsets/bin/7bit.hcchr 'p2shwpkh:m/49h/0h/0h/0/0:army excuse hero wolf disease liberty moral diagram ? ? ? ?:3HX5tttedDehKWTTGpxaPAbo157fnjn89s' '?1?2?1?2?1?2?3'" >> run.sh
else
  echo "Running for brain CLIENT $SSH_IP port $SSH_PORT"
  read -p "Press enter to continue"
  #echo "./hashcat.bin -m 28510 --brain-client --brain-password $BRAIN_PASSWORD --brain-host $BRAIN_IP --brain-port $BRAIN_EXTERNAL_PORT -w 4 --status --self-test-disable -a 3 -1 charsets/bin/5bit.hcchr -2 charsets/bin/6bit.hcchr -3 charsets/bin/7bit.hcchr 'p2shwpkh:m/49h/0h/0h/0/0:army excuse hero wolf disease liberty moral diagram ? ? ? ?:3HX5tttedDehKWTTGpxaPAbo157fnjn89s' '?1?2?1?2?1?2?3'" >> run.sh
fi

chmod +x run.sh
scp -P $SSH_PORT run.sh root@$SSH_IP:/root/
scp -P $SSH_PORT bitcoin-recovery-tool.7z root@$SSH_IP:/root/
ssh -p $SSH_PORT root@$SSH_IP -L 8080:localhost:8080

./hashcat.bin -m 16600 -a 7 '$electrum$2*8285fce2ea64c60e8318f58da0b84cf6*7314cf8b1c22077c786004f32a801ab2' --increment --increment-min 7 --increment-max 16 '?l?l?l?l?l?l?l?l?l?l?l?l?l?l?l?l?l?l?l?l' dict.txt
