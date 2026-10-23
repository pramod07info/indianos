//! IndianOS x86_64 ring-3 native application SDK (experimental v1).
//! Never call Linux/Windows APIs; INT 0x80 is the IndianOS user ABI.
//! These functions are only valid inside an IndianOS ring-3 application.
#![no_std]

use core::arch::asm;
pub const ABI_VERSION:u64=1;
pub const MAX_WRITE_BYTES:usize=256;
pub const BAD_RESULT:u64=u64::MAX;

#[inline(always)]
pub fn raw_syscall(number:u64,arg0:u64,arg1:u64)->u64{
    #[cfg(target_arch="x86_64")]
    {
        let result:u64;
        unsafe{
            asm!("int 0x80",inlateout("rax")number=>result,
                in("rdi")arg0,in("rsi")arg1,options(nostack));
        }
        result
    }
    #[cfg(not(target_arch="x86_64"))]
    {
        let _=(number,arg0,arg1);
        BAD_RESULT
    }
}
pub fn abi_supported()->bool{
    raw_syscall(0,0,0)==ABI_VERSION
}
pub fn write(bytes:&[u8])->bool{
    !bytes.is_empty() && bytes.len()<=MAX_WRITE_BYTES &&
        raw_syscall(1,bytes.as_ptr() as u64,bytes.len() as u64)==bytes.len() as u64
}
pub fn yield_now()->bool{
    raw_syscall(3,0,0)==0
}
pub fn exit(status:u64)->!{
    let _=raw_syscall(2,status,0);
    loop{core::hint::spin_loop();}
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn syscall_contract_constants(){
        assert_eq!(ABI_VERSION,1);
        assert_eq!(MAX_WRITE_BYTES,256);
        assert_eq!(BAD_RESULT,u64::MAX);
    }
}
