# Forge Container
## Project Summary
This is my attempt of implementing a virtualization container, similar to Docker.  I decided to implement this in Rust as I wanted to gain familiarity with a common new low level programming language that I have not used before.  The key things that had to be tackled including creating a run-time container by isolating namespaces.  

I spawn an ubuntu VM using multipass due to my reliance on specific linux commands.  I then worked to setup a filesystem, utilize cgroups to limit the resources consumed by my container, and copy over binaries to allow my container to run commands such as ls and ip.  I also needed to setup network access for the container through network interfacing.

The last step was to be able to handling imaging so users could leverage the container to run code inside it, the real power of containers.

## Phase 1 - Isolating Processes
The first goal was to be able to run my container, and spawn a process with a pid of 1, without showing the pids of the host.  This would show that I have correctly isolated processes within the container. To do this we use `fork()` to copy the process and `unshare()` to be able to create new linux namespaces for the process. We need to use new namespaces as it is the Linux kernel's way of providing processes with isolated views of system resources.  When initializing the name spaces we first strive to isolate process tree, mounted filesystems, and hostname.  We leverage the namespace flags `CLONE_NEWPID`, `CLONE_NEWNS`, and `CLONE_NEWUTS` for this combined with a pipe operator.

#### Testing Phase 1
In the container shell we execute the following command:
`echo $$`
Which yields the pid of 1. 

We then run the same command not within the container but simply within the VM shell, we show a far higher pid

#### Key Aspects For Phase 1
We leverage several system calls to enable phase 1 to be possible, these include 
|Command|Purpose|
|-------|-------|
|`fork()`| Create copy of the current process|
|`unshare()`| Create new namespaces for the process|
|`execvp()`| Replace the current process with bash|
|`waitpid()`| Wait for child process to finish in parent|

One key thing to note is the use of 2 `fork()` commands.  The reason for this is we have our main running on the host which has its own pid (say 123) which then spawns a child on the host (say pid 124).  We then call `unshare()` to create the new namespace for pid 124 and `fork()` again to create a grandchild with pid 1 in the new namespace. `exec bash` then becomes pid 1 shell.

## Phase 2 - Isolating Filesystem
The goal here is to mount a new root filesystem for the container.  Here we need to mount folders such as `/proc` so we can leverage the `ps` command.  The objective is to create a root for the container and pivot the root to change the container's view of the filesystem.  We will also set up other directories such as `/bin`, `/lib` and `/etc`.  In addition to setting up basic file system directories we had to copy over binaries from the host such as `/bin/bash` for the shell and utilities such as `/bin/ls` as once we isolate the filesystem we would no longer to be able to run these pivotal commands from the container

#### Testing Phase 2 - Further Process Isolation
To bein we test more process isolation (we couldn't do this before as `/proc` was not mounted) the container shell we execute the following command `ps aux`, which only shows 2 processes.


We then run the same command not within the container but simply within the VM shell, showing far more processes, proving we are isolated. 

#### Testing Phase 2 - Filesystem Isolation
Prior to setting up our filesystem namespace we were able from the container execute commands such as `ls /home` which would then list the files and directories in the home folder on the host VM.  After making the changes when running the same command `ls /home` we return empty as the container no longer has visibility into the host

#### Use of pivot_root vs. chroot
|Feature|chroot|pivot_root|
|-------|------|----------|
|Security|Could escape with sufficient priviledge| Can't escape|
|Root Mount| Changes apparent root| Changes actual root mount|
|Old root| Still accessible | Removed completely|
|Use Case| Dev/Testing | Production container|

Docker utilizes `pivot_root` as well.

#### Key Aspects For Phase 2
- Learning virtual filesystems in linux.
- Dealing with shared libraries (`.so` files)
- Dynamic linking with `ldd`
- Mount points and binding

## Phase 3 - Employing Resource Limits
The current structure allows the container to consume an unlimited amount of resources from the host system.  This is an issue as it can consume too much CPU, starving other processes, consume too much RAM, crashing the system, consume too much storage, filling disk space, or spawn unlimited processes, fork bomb.  We solve this with linux cgroups.

#### Testing Phase 3
Here we test each aspect individually by leveraging one terminal within the container and one terminal in the host outside the container. 

##### Testing CPU Use
Within the container, we can run `while true; do :; done ` to try and consume all the CPU.  On the host terminal we then run `ps aux` to show that the container process is taking up around our CPU limit of 50% instead of 100%, highlighting our cgroup CPU limit working.

##### Testing Memory Use
Within the container we can run `dd if=/dev/zero of=/tmp/file bs=1G` to consume all the ram, but then looking on our host system we can see it stops when it reaches the RAM allocated by our cgroup.

##### Testing Storage Limit
Here we attempt to write a file exceeding out 512MB limit and see that it fails.  For example `dd if=/dev/zero of=/tmp/bigfile bs=1M count=600`

##### Testing Process Limit
Here we can run `:(){ :|:& };: ` to fork bomb.  But if we monitor the number of processes, we see they never exceed 100 which is our cgroup implemented limit.

#### Key Aspects For Phase 3
We autodetect if cgroup v1 or cgroup v2 is active to provide working limits along both philosophies.  There is a different file structure for cgroups between the 2 verisons making this crucial if you want to be able to run this along both methods. It was also key to set up the cgroups after our second fork but prior to setting up our virtual filesystem 

## Phase 4 - Network Isolation
Currently the container can see all host network traffic.  In addition to this not being compliant with the container paradigm this creates practical issues, such as if the container binds to port 80, the host can no longer utilize that port.  This is also a security issue as the container would be able to sniff host network traffic.  

What we should do here is create a network namespace just for the container, then use a virtual ethernet (veth) pair to connect the container to the host.  The container will have its own IP address, can communicate with the host and the internet, and be isolated from other containers.

With the veth pair we can have one end of our virtual cable connected to the host, and one end connected to the container just like if you were to connect a physical ethernet cable between 2 machines.  

To do this we are going to leverage the `CLONE_NEWNET` flag for namespaces.

#### High Level Network Isolation Steps
##### Step 1 - Create The Pipe Using The veth Pair
[veth-7003] ←──pipe──→ [veth-c-7003]
   (host)                 (container)

##### Step 2 - Move One End Of Pipe Into Container
Host room:                Container room:
[veth-7003]               [veth-c-7003]
     ↓                          ↓
  10.0.0.1                  10.0.0.2

##### Step 3 - Configure IP Address
Host side:        Container side:
10.0.0.1/24       10.0.0.2/24

##### Step 4 - Set Default Route In Container
Container: "To reach the internet, send packets to 10.0.0.1"

##### Step 5 - Enable NAT On Host
This allows the internet to think it is talking to the host not the container
Container (10.0.0.2) → Host translates → Internet (sees host's IP)
Internet response → Host translates back → Container (10.0.0.2)

##### Step 6 - iptables Forward Rules
Tell host firewall to let packets flow between container and internet
Allow packets: veth-7003 ←→ enp0s1

#### Journey Of A Packet
When container runs `ping 8.8.8.8` 

1. Container: "I want to reach 8.8.8.8"
   
2. Container checks route table: "Not 10.0.0.x, use default → 10.0.0.1"
   
3. Packet goes through veth-c-7003 → veth-7003 (the pipe)
   Source: 10.0.0.2
   Dest: 8.8.8.8
   
4. Host receives packet on veth-7003
   
5. Host checks: "8.8.8.8 not local, need to forward"
   
6. iptables FORWARD rule: "Allow veth-7003 → enp0s1" ✓
   
7. iptables NAT (MASQUERADE): 
   Changes source from 10.0.0.2 → 192.168.2.31 (host IP)
   
8. Packet goes out enp0s1 to internet
   Source: 192.168.2.31
   Dest: 8.8.8.8
   
9. Internet sees packet from 192.168.2.31, responds
   
10. Response comes back to host (192.168.2.31)
    
11. Host's NAT remembers: "This is for 10.0.0.2"
    Changes dest from 192.168.2.31 → 10.0.0.2
    
12. iptables FORWARD: "Allow enp0s1 → veth-7003" ✓
    
13. Packet goes through veth-7003 → veth-c-7003
    
14. Container receives ping response! 🎉





