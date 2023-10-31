# Uploads the release to a cloud instance with a script to execute
# Requires ssh support by the server (tested on vast.ai)

NAME="bitcoin-recovery-tool"
set -e

[ -z $1 ] && echo "You must supply the IP:PORT of the instance" && exit 1
SSH_IP=`echo $1 | cut -d':' -f1`
SSH_PORT=`echo $1 | cut -d':' -f2`

scp -P $SSH_PORT $NAME.zip root@$SSH_IP:/root/
ssh -p $SSH_PORT root@$SSH_IP -L 8080:localhost:8080 "sudo apt-get install unzip"
ssh -p $SSH_PORT root@$SSH_IP -L 8080:localhost:8080 "unzip $NAME"
ssh -p $SSH_PORT root@$SSH_IP -L 8080:localhost:8080
