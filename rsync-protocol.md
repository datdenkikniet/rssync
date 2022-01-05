# All exchanges
Client: initialisation -> `@RSYNCD: MAJ.MIN\n`
Server: initialisation -> `@RSYNCD: MAJ.MIN\n`

# Module listing
Client: request module listing -> `\n`
Server: respond with module listing -> `<formatted module names separated by \n>`
Server: exit -> `@RSYNCD: EXIT`

start_daemon
|- exchange_protocols
|- send_listing
|- lp_number
|- rsync_module
 |- read_args
 |- setup_protocol
 |- start_server
  |- recv_filter_list
   |- read_sbuf
   |- read_a_msg
  |- do_server_sender
   |- send_file_list
    |- send_file_name
     |- send_file_entry
    |- write_end_of_flist
   |- send_files
   |- read_final_goodbye

Exchange is:
* Exchange version numbers (newline terminated)
* Read query or something like that (newline terminated)
* Send OK (newline terminated)
* Read client arguments (null-terminated)
* Send compatibility flags (as one byte, or sometimes as varint?)
* Send checksum seed (4 bytes)
* Send file list

Varints:
Assume we start with 0x00000000DEADBEEF

state:
```
  buf = [0, 0, 0, 0, 0, 0, 0, 0, 0]
  x = 0x00000000DEADBEEF
  min_bytes = 3
  b = [0, 0, 0, 0, 0, 0, 0, 0, 0]
  bit = 0
  cnt = 8      
```

move all bytes of the value to index 1 in the buffer by running SIVAL64

changed state:
```
  b = [0, 0xEF, 0xBE, 0xAD, 0xDE, 0x00, 0x00, 0x00, 0x00]
```

Subtract the amount of nonzero trailing null bytes from cnt

changed state:
```
  cnt = 4
```

set bit to 1 left shifted by seven minus the amount of nonzero bytes plus the minimum amount of bytes, which
is the same as two to the power of the amount of leftover zero-valued bytes in b

changed state:
```
  bit = 1 << (7 - 4 + 3) = 1 << 6 = 0x40
```

if the last byte to be transmitted is larger than or equal to bit, add another nonzero byte to send and
set the first element of b equal to it. This is true in this case, as
0xDE >= 0x40

changed state:
```
  cnt = 5
  bit = ~(bit - 1) = ~(0x40 - 1) = ~(0x3F) = 0b11000000 = 0xC0
  b = [0xC0, 0xEF, 0xBE, 0xAD, 0xDE, 0x00, 0x00, 0x00, 0x00]
```

  
