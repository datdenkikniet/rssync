# rssync: an rsync clone in rust
**rssync** aims to be an rsync clone in rust. 

Initially, the aim is to create a daemon that only support the sending of files, using version 31 of the rsync protocol. This is done to reduce the scope, and because
it is the part that the author (@datdenkikniet) actually would like to make some changes to.