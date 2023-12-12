# Renting GPUs
To rent GPUs we recommend using [Vast.ai](https://vast.ai/) which allows you to easily rent powerful GPUs by paying in bitcoin.

- First you need to create and fund an account
- In [templates](https://cloud.vast.ai/templates/), select `Cuda:12.0.1-Devel-Ubuntu20.04` 
- In [search](https://cloud.vast.ai/create/), find a cluster to rent (for additional security select "Secure Cloud" under "Machine Options")
- In [instances](https://cloud.vast.ai/instances/), wait for the cluster to start then select "Connect" to find the "Open Ports" (e.g. `86.127.240.108:25128`)

Run commands to copy the `seedcat` zip file to the machine, replacing the IP and port with your rented ones:
```bash
SSH_IP="86.127.240.108" # CHANGE THIS
SSH_PORT="25128"  # CHANGE THIS
scp -P $SSH_PORT seedcat*.zip root@$SSH_IP:/root/
ssh -p $SSH_PORT root@$SSH_IP -L 8080:localhost:8080
```

Then once you SSH into the machine run the following:
```
sudo apt-get install unzip -y
unzip seedcat*zip
cd seedcat
```

Now you can run `./seedcat` commands which will use the GPUs on the rented machine!