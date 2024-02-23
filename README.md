# DelProtect-rs

Rust minifilter based on https://github.com/zodiacon/windowskernelprogrammingbook/tree/master/chapter09/SysMon

###Directory hierarchy
**delprotect-km** - minifilter project which gather allows to block particular deletes

**delprotect-um** - user mode program to configure minifilter

**common** - shared info between driver and client, like ioctl codes

### How to use
#### Installing (with admin rights):
Click right mouse button on DelProtect.inf and choose install or type
> RUNDLL32.EXE SETUPAPI.DLL,InstallHinfSection DefaultInstall 132 C:\VsExclude\kernel\delprotect\delprotect.inf

#### Start: 
> fltmc load minifilter

#### Setup minifilter:
To block deletes from cmd.exe
> delprotect-client.exe add cmd.exe

To clear list of prevented deletes
> delprotect-client.exe clear

#### Stop:
> fltmc unload minifilter