# SysMon-rs

Rust driver based on https://github.com/zodiacon/windowskernelprogrammingbook/tree/master/chapter09/SysMon

###Directory hierarchy
**sysmon-km** - driver project which gather particular events from system

**sysmon-um** - user mode program to read and display events saved by driver

**common** - shared info between driver and client, like format of data send from driver to client

### How to use
Installing (with admin rights):
> sc create sysmon type=kernel binPath=<driver.sys path>

Start: 
> sc start sysmon

Read events saved in driver:
> sysmon-client.exe

Stop:
> sc stop sysmon