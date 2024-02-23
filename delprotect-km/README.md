DelProtect

delprotect-minifilter-rust - rust minifilter based on https://github.com/zodiacon/windowskernelprogrammingbook/tree/master/chapter10/DelProtect3

Installing <br>
Click on .inf file and choose install or type in cmd (with admin rights)
> RUNDLL32.EXE SETUPAPI.DLL,InstallHinfSection DefaultInstall 132 <delprotect inf path>

Start: 
> ltmc load delprotect

Stop:
> ltmc unload delprotect