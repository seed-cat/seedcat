# Renting GPUs
Below are the instructions for running `seedcat` on [Vast.ai](https://vast.ai/) on Linux or iOS:

### Setup
1. [Create an account](https://vast.ai/docs/console/introduction) with an email address and password
2. [Fund the account](https://cloud.vast.ai/billing/) by clicking `Add Credit` (you can pay in bitcoin if you like)
3. Generate an ssh-key by running the following commands:
```bash
ssh-keygen -t rsa -v -f "$HOME/.ssh/id_rsa" -N ""
cat $HOME/.ssh/id_rsa.pub
```

This command will print out a long ssh pubkey that looks like this:
```bash
# Example output from the step above
ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQDu8... name@os
```

4. [Log into your account](https://cloud.vast.ai/account) then click `ADD SSH KEY` and paste in the key above

### Running Recovery
1. [Choose an instance to create](https://cloud.vast.ai/create/) then click `Change Template` and select `Cuda:12.0.1-Devel-Ubuntu20.04`
2. We recommend you click `RENT` on a `8x RTX 4090` instance for maximum speed and choose a `datacenter` instance for better security
3. [Go to your instance](https://cloud.vast.ai/instances/) and click on `Connect` and copy the `Direct ssh connect`
4. Paste the command into your terminal, it will look something like this:
```bash
# Example command from the step above
ssh -p 19879 root@140.228.20.3 -L 8080:localhost:8080
```
5. Once your terminal changes to something like `root@C.14891369:~$` to indicate you are logged into the remote instance, then paste in the following commands:
```bash
# Change this to the latest version if you like
VERSION=0.0.2

# Get the seedcat binaries
wget https://github.com/seed-cat/seedcat/releases/download/v$VERSION/seedcat_$VERSION.zip
wget https://github.com/seed-cat/seedcat/releases/download/v$VERSION/seedcat_$VERSION.zip.sig
sudo apt-get install unzip pgp -y
```

6. Verify the signatures and unzip the seedcat binaries like so:
```
# Verify signatures and run seedcat
gpg --keyserver keyserver.ubuntu.com --recv-keys D249C16D6624F2C1DD0AC20B7E1F90D33230660A
gpg --verify seedcat_$VERSION.zip.sig || exit

unzip seedcat_$VERSION.zip
cd seedcat
./seedcat
```

For instructions on how to use seedcat [see the documentation](recovery.md)
