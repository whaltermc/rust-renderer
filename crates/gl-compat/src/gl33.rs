//! OpenGL 3.3 compatibility surface for an OpenGL ES 3.x driver.
//!
//! The GLES 3.x API is already close to desktop GL 3.3 for buffers, VAOs,
//! samplers, FBOs and instanced drawing.  This module supplies the desktop
//! entry points that are missing from the Android GLES ABI and translates the
//! small set of desktop-only state/format differences.  Features with no
//! GLES 3.x equivalent are rejected with a real GL error rather than silently
//! pretending to work.

use std::ffi::{c_char, c_void};
use std::ptr;

const GL_INVALID_ENUM: u32 = 0x0500;
const GL_INVALID_VALUE: u32 = 0x0501;
const GL_INVALID_OPERATION: u32 = 0x0502;
const GL_SAMPLES_PASSED: u32 = 0x8914;
const GL_TIME_ELAPSED: u32 = 0x88BF;
const GL_QUERY_COUNTER_BITS: u32 = 0x8864;
const GL_SYNC_GPU_COMMANDS_COMPLETE: u32 = 0x9117;
const GL_WAIT_FAILED: u32 = 0x911D;

fn err(e: u32) { crate::errors().set(e); }

unsafe fn f<T: Copy>(name: &str) -> Option<T> { crate::driver_fn::<T>(name) }

unsafe fn call_void1(name: &str, a: u32) -> bool {
    if let Some(x) = f::<unsafe extern "C" fn(u32)>(name) { x(a); true } else { false }
}

#[no_mangle]
pub unsafe extern "C" fn glBindFragDataLocation(program: u32, color: u32, name: *const c_char) {
    // GLES 3.0 has explicit fragment output locations in GLSL ES 3.00, so
    // the shader translator normally removes the need for this desktop call.
    // When possible, resolve it through the driver; otherwise ignore only
    // color 0 and report an error for other locations.
    if let Some(x) = f::<unsafe extern "C" fn(u32,u32,*const c_char)>("glBindFragDataLocation") { x(program,color,name); return; }
    if color != 0 { err(GL_INVALID_OPERATION); }
}

#[no_mangle]
pub unsafe extern "C" fn glBindSampler(unit: u32, sampler: u32) {
    if !call_void2("glBindSampler", unit, sampler) { err(GL_INVALID_OPERATION); }
}

unsafe fn call_void2(name: &str, a: u32, b: u32) -> bool {
    if let Some(x) = f::<unsafe extern "C" fn(u32,u32)>(name) { x(a,b); true } else { false }
}

#[no_mangle]
pub unsafe extern "C" fn glGenSamplers(n: i32, samplers: *mut u32) {
    if n < 0 || samplers.is_null() { err(GL_INVALID_VALUE); return; }
    if let Some(x) = f::<unsafe extern "C" fn(i32,*mut u32)>("glGenSamplers") { x(n,samplers); return; }
    err(GL_INVALID_OPERATION);
}
#[no_mangle]
pub unsafe extern "C" fn glDeleteSamplers(n: i32, samplers: *const u32) {
    if n < 0 { err(GL_INVALID_VALUE); return; }
    if let Some(x) = f::<unsafe extern "C" fn(i32,*const u32)>("glDeleteSamplers") { x(n,samplers); return; }
    err(GL_INVALID_OPERATION);
}
#[no_mangle]
pub unsafe extern "C" fn glIsSampler(sampler: u32) -> u8 {
    if let Some(x) = f::<unsafe extern "C" fn(u32)->u8>("glIsSampler") { return x(sampler); }
    0
}
#[no_mangle]
pub unsafe extern "C" fn glSamplerParameteri(sampler: u32, pname: u32, param: i32) {
    if let Some(x) = f::<unsafe extern "C" fn(u32,u32,i32)>("glSamplerParameteri") { x(sampler,pname,param); } else { err(GL_INVALID_OPERATION); }
}
#[no_mangle]
pub unsafe extern "C" fn glSamplerParameterf(sampler: u32, pname: u32, param: f32) {
    if let Some(x) = f::<unsafe extern "C" fn(u32,u32,f32)>("glSamplerParameterf") { x(sampler,pname,param); } else { err(GL_INVALID_OPERATION); }
}
#[no_mangle]
pub unsafe extern "C" fn glSamplerParameteriv(sampler: u32, pname: u32, p: *const i32) {
    if let Some(x) = f::<unsafe extern "C" fn(u32,u32,*const i32)>("glSamplerParameteriv") { x(sampler,pname,p); } else { err(GL_INVALID_OPERATION); }
}
#[no_mangle]
pub unsafe extern "C" fn glSamplerParameterfv(sampler: u32, pname: u32, p: *const f32) {
    if let Some(x) = f::<unsafe extern "C" fn(u32,u32,*const f32)>("glSamplerParameterfv") { x(sampler,pname,p); } else { err(GL_INVALID_OPERATION); }
}
#[no_mangle]
pub unsafe extern "C" fn glGetSamplerParameteriv(sampler: u32, pname: u32, p: *mut i32) {
    if let Some(x) = f::<unsafe extern "C" fn(u32,u32,*mut i32)>("glGetSamplerParameteriv") { x(sampler,pname,p); } else if !p.is_null() { *p=0; err(GL_INVALID_OPERATION); }
}
#[no_mangle]
pub unsafe extern "C" fn glGetSamplerParameterfv(sampler: u32, pname: u32, p: *mut f32) {
    if let Some(x) = f::<unsafe extern "C" fn(u32,u32,*mut f32)>("glGetSamplerParameterfv") { x(sampler,pname,p); } else if !p.is_null() { *p=0.0; err(GL_INVALID_OPERATION); }
}

#[no_mangle]
pub unsafe extern "C" fn glVertexAttribDivisor(index: u32, divisor: u32) {
    if let Some(x)=f::<unsafe extern "C" fn(u32,u32)>("glVertexAttribDivisor") { x(index,divisor); } else { err(GL_INVALID_OPERATION); }
}
#[no_mangle]
pub unsafe extern "C" fn glDrawArraysInstanced(mode:u32,first:i32,count:i32,primcount:i32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,i32,i32,i32)>("glDrawArraysInstanced"){x(mode,first,count,primcount)}else{err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glDrawElementsInstanced(mode:u32,count:i32,ty:u32,indices:*const c_void,primcount:i32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,i32,u32,*const c_void,i32)>("glDrawElementsInstanced"){x(mode,count,ty,indices,primcount)}else{err(GL_INVALID_OPERATION)}
}

// GL 3.3's double-precision vertex attribute API has no GLES 3.0 equivalent.
// Reject it explicitly rather than forwarding an incompatible ABI.
#[no_mangle]
pub unsafe extern "C" fn glVertexAttribLPointer(_index:u32,_size:i32,_ty:u32,_stride:i32,_ptr:*const c_void){err(GL_INVALID_OPERATION)}
#[no_mangle]
pub unsafe extern "C" fn glGetVertexAttribLdv(_index:u32,_pname:u32,params:*mut f64){if !params.is_null(){*params=0.0;}err(GL_INVALID_OPERATION)}

// Packed normalized attribute formats are GLES-compatible through the generic
// vertexAttribPointer path for the common unsigned-byte/short forms.
#[no_mangle] pub unsafe extern "C" fn glVertexAttribP1uiv(index:u32,type_:u32,normalized:u8,value:*const u32){ packed_attrib(index,1,type_,normalized,value); }
#[no_mangle] pub unsafe extern "C" fn glVertexAttribP2uiv(index:u32,type_:u32,normalized:u8,value:*const u32){ packed_attrib(index,2,type_,normalized,value); }
#[no_mangle] pub unsafe extern "C" fn glVertexAttribP3uiv(index:u32,type_:u32,normalized:u8,value:*const u32){ packed_attrib(index,3,type_,normalized,value); }
#[no_mangle] pub unsafe extern "C" fn glVertexAttribP4uiv(index:u32,type_:u32,normalized:u8,value:*const u32){ packed_attrib(index,4,type_,normalized,value); }
unsafe fn packed_attrib(index:u32,size:i32,type_:u32,normalized:u8,value:*const u32){
    if value.is_null(){err(GL_INVALID_VALUE);return}
    // GL_INT_2_10_10_10_REV and GL_UNSIGNED_INT_2_10_10_10_REV are not
    // accepted by every GLES 3 driver. Prefer the native entry point when present.
    let name=match size {1=>"glVertexAttribP1uiv",2=>"glVertexAttribP2uiv",3=>"glVertexAttribP3uiv",_=>"glVertexAttribP4uiv"};
    if let Some(x)=f::<unsafe extern "C" fn(u32,u32,u8,*const u32)>(name){x(index,type_,normalized,value)}else{err(GL_INVALID_OPERATION)}
}

#[no_mangle]
pub unsafe extern "C" fn glClearBufferiv(buffer:u32,drawbuffer:i32,value:*const i32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,i32,*const i32)>("glClearBufferiv"){x(buffer,drawbuffer,value)}else{err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glClearBufferuiv(buffer:u32,drawbuffer:i32,value:*const u32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,i32,*const u32)>("glClearBufferuiv"){x(buffer,drawbuffer,value)}else{err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glClearBufferfv(buffer:u32,drawbuffer:i32,value:*const f32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,i32,*const f32)>("glClearBufferfv"){x(buffer,drawbuffer,value)}else{err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glClearBufferfi(buffer:u32,drawbuffer:i32,depth:f32,stencil:i32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,i32,f32,i32)>("glClearBufferfi"){x(buffer,drawbuffer,depth,stencil)}else{err(GL_INVALID_OPERATION)}
}

#[no_mangle]
pub unsafe extern "C" fn glFramebufferTextureLayer(target:u32,attachment:u32,texture:u32,level:i32,layer:i32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,u32,u32,i32,i32)>("glFramebufferTextureLayer"){x(target,attachment,texture,level,layer)}else{err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glBlitFramebuffer(sx0:i32,sy0:i32,sx1:i32,sy1:i32,dx0:i32,dy0:i32,dx1:i32,dy1:i32,mask:u32,filter:u32){
    if let Some(x)=f::<unsafe extern "C" fn(i32,i32,i32,i32,i32,i32,i32,i32,u32,u32)>("glBlitFramebuffer"){x(sx0,sy0,sx1,sy1,dx0,dy0,dx1,dy1,mask,filter)}else{err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glRenderbufferStorageMultisample(target:u32,samples:i32,internalformat:u32,w:i32,h:i32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,i32,u32,i32,i32)>("glRenderbufferStorageMultisample"){x(target,samples,internalformat,w,h)}else{err(GL_INVALID_OPERATION)}
}

#[no_mangle]
pub unsafe extern "C" fn glTexStorage1D(target:u32,levels:i32,internalformat:u32,width:i32){
    // GLES 3.0 has no 1D textures. Desktop 1D is best represented by a 2D
    // texture of height one, but changing the target breaks texture bindings,
    // so reject it instead of corrupting state.
    let _=(target,levels,internalformat,width);err(GL_INVALID_ENUM);
}
#[no_mangle]
pub unsafe extern "C" fn glTexStorage2D(target:u32,levels:i32,internalformat:u32,width:i32,height:i32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,i32,u32,i32,i32)>("glTexStorage2D"){x(target,levels,internalformat,width,height)}else{err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glTexStorage3D(target:u32,levels:i32,internalformat:u32,width:i32,height:i32,depth:i32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,i32,u32,i32,i32,i32)>("glTexStorage3D"){x(target,levels,internalformat,width,height,depth)}else{err(GL_INVALID_OPERATION)}
}

#[no_mangle]
pub unsafe extern "C" fn glGetInteger64v(pname:u32,data:*mut i64){
    if let Some(x)=f::<unsafe extern "C" fn(u32,*mut i64)>("glGetInteger64v"){x(pname,data)}else if !data.is_null(){*data=0;err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glGetIntegeri_v(pname:u32,index:u32,data:*mut i32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,u32,*mut i32)>("glGetIntegeri_v"){x(pname,index,data)}else if !data.is_null(){*data=0;err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glGetInteger64i_v(pname:u32,index:u32,data:*mut i64){
    if let Some(x)=f::<unsafe extern "C" fn(u32,u32,*mut i64)>("glGetInteger64i_v"){x(pname,index,data)}else if !data.is_null(){*data=0;err(GL_INVALID_OPERATION)}
}

// Query objects: GLES 3.0 supports the useful occlusion/transform-feedback
// query targets, so these are direct ABI-compatible forwards.
#[no_mangle]
pub unsafe extern "C" fn glGenQueries(n:i32,ids:*mut u32){if let Some(x)=f::<unsafe extern "C" fn(i32,*mut u32)>("glGenQueries"){x(n,ids)}else{err(GL_INVALID_OPERATION)}}
#[no_mangle]
pub unsafe extern "C" fn glDeleteQueries(n:i32,ids:*const u32){if let Some(x)=f::<unsafe extern "C" fn(i32,*const u32)>("glDeleteQueries"){x(n,ids)}else{err(GL_INVALID_OPERATION)}}
#[no_mangle]
pub unsafe extern "C" fn glIsQuery(id:u32)->u8{if let Some(x)=f::<unsafe extern "C" fn(u32)->u8>("glIsQuery"){x(id)}else{0}}
#[no_mangle]
pub unsafe extern "C" fn glBeginQuery(target:u32,id:u32){if target==GL_TIME_ELAPSED||target==GL_SAMPLES_PASSED{err(GL_INVALID_ENUM)}else if let Some(x)=f::<unsafe extern "C" fn(u32,u32)>("glBeginQuery"){x(target,id)}else{err(GL_INVALID_OPERATION)}}
#[no_mangle]
pub unsafe extern "C" fn glEndQuery(target:u32){if target==GL_TIME_ELAPSED||target==GL_SAMPLES_PASSED{err(GL_INVALID_ENUM)}else if let Some(x)=f::<unsafe extern "C" fn(u32)>("glEndQuery"){x(target)}else{err(GL_INVALID_OPERATION)}}
#[no_mangle]
pub unsafe extern "C" fn glQueryCounter(id:u32,target:u32){let _=(id,target);err(GL_INVALID_OPERATION)}
#[no_mangle]
pub unsafe extern "C" fn glGetQueryiv(target:u32,pname:u32,params:*mut i32){if let Some(x)=f::<unsafe extern "C" fn(u32,u32,*mut i32)>("glGetQueryiv"){x(target,pname,params)}else if !params.is_null(){*params=0;err(GL_INVALID_OPERATION)}}
#[no_mangle]
pub unsafe extern "C" fn glGetQueryObjectiv(id:u32,pname:u32,params:*mut i32){if let Some(x)=f::<unsafe extern "C" fn(u32,u32,*mut i32)>("glGetQueryObjectiv"){x(id,pname,params)}else if !params.is_null(){*params=0;err(GL_INVALID_OPERATION)}}
#[no_mangle]
pub unsafe extern "C" fn glGetQueryObjectuiv(id:u32,pname:u32,params:*mut u32){if let Some(x)=f::<unsafe extern "C" fn(u32,u32,*mut u32)>("glGetQueryObjectuiv"){x(id,pname,params)}else if !params.is_null(){*params=0;err(GL_INVALID_OPERATION)}}
#[no_mangle]
pub unsafe extern "C" fn glGetQueryObjecti64v(_id:u32,_pname:u32,params:*mut i64){if !params.is_null(){*params=0;}err(GL_INVALID_OPERATION)}
#[no_mangle]
pub unsafe extern "C" fn glGetQueryObjectui64v(_id:u32,_pname:u32,params:*mut u64){if !params.is_null(){*params=0;}err(GL_INVALID_OPERATION)}

// Sync objects are present in GLES 3.0 with the same ABI.
#[no_mangle]
pub unsafe extern "C" fn glFenceSync(condition:u32,flags:u32)->*const c_void{if condition!=GL_SYNC_GPU_COMMANDS_COMPLETE||flags!=0{err(GL_INVALID_VALUE);return ptr::null()}if let Some(x)=f::<unsafe extern "C" fn(u32,u32)->*const c_void>("glFenceSync"){x(condition,flags)}else{err(GL_INVALID_OPERATION);ptr::null()}}
#[no_mangle]
pub unsafe extern "C" fn glDeleteSync(sync:*const c_void){if let Some(x)=f::<unsafe extern "C" fn(*const c_void)>("glDeleteSync"){x(sync)}else{err(GL_INVALID_OPERATION)}}
#[no_mangle]
pub unsafe extern "C" fn glIsSync(sync:*const c_void)->u8{if let Some(x)=f::<unsafe extern "C" fn(*const c_void)->u8>("glIsSync"){x(sync)}else{0}}
#[no_mangle]
pub unsafe extern "C" fn glClientWaitSync(sync:*const c_void,flags:u32,timeout:u64)->u32{if let Some(x)=f::<unsafe extern "C" fn(*const c_void,u32,u64)->u32>("glClientWaitSync"){x(sync,flags,timeout)}else{err(GL_INVALID_OPERATION);GL_WAIT_FAILED}}
#[no_mangle]
pub unsafe extern "C" fn glWaitSync(sync:*const c_void,flags:u32,timeout:u64){if let Some(x)=f::<unsafe extern "C" fn(*const c_void,u32,u64)>("glWaitSync"){x(sync,flags,timeout)}else{err(GL_INVALID_OPERATION)}}
#[no_mangle]
pub unsafe extern "C" fn glGetSynciv(sync:*const c_void,pname:u32,count:i32,length:*mut i32,values:*mut i32){if let Some(x)=f::<unsafe extern "C" fn(*const c_void,u32,i32,*mut i32,*mut i32)>("glGetSynciv"){x(sync,pname,count,length,values)}else{err(GL_INVALID_OPERATION)}}

// GL 3.3 indexed buffer bindings are ES 3.0 compatible.
#[no_mangle]
pub unsafe extern "C" fn glBindBufferBase(target:u32,index:u32,buffer:u32){if let Some(x)=f::<unsafe extern "C" fn(u32,u32,u32)>("glBindBufferBase"){x(target,index,buffer)}else{err(GL_INVALID_OPERATION)}}
#[no_mangle]
pub unsafe extern "C" fn glBindBufferRange(target:u32,index:u32,buffer:u32,offset:isize,size:isize){if let Some(x)=f::<unsafe extern "C" fn(u32,u32,u32,isize,isize)>("glBindBufferRange"){x(target,index,buffer,offset,size)}else{err(GL_INVALID_OPERATION)}}

// ES 3 has no GL_CLAMP border mode; texture parameters are translated by the
// main glTexParameteri wrapper. This helper is intentionally exported for the
// resolver so aliases can share one implementation.



#[no_mangle]
pub unsafe extern "C" fn glDrawRangeElements(mode:u32,start:u32,end:u32,count:i32,ty:u32,indices:*const c_void){
    if let Some(x)=f::<unsafe extern "C" fn(u32,u32,u32,i32,u32,*const c_void)>("glDrawRangeElements"){x(mode,start,end,count,ty,indices)}else if let Some(x)=f::<unsafe extern "C" fn(u32,i32,u32,*const c_void)>("glDrawElements"){x(mode,count,ty,indices)}else{err(GL_INVALID_OPERATION)}
}

#[no_mangle]
pub unsafe extern "C" fn glMultiDrawArrays(mode:u32,first:*const i32,count:*const i32,drawcount:i32){
    if drawcount < 0 || (drawcount > 0 && (first.is_null() || count.is_null())) { err(GL_INVALID_VALUE); return; }
    if let Some(x)=f::<unsafe extern "C" fn(u32,*const i32,*const i32,i32)>("glMultiDrawArrays"){x(mode,first,count,drawcount);return}
    let Some(draw)=f::<unsafe extern "C" fn(u32,i32,i32)>("glDrawArrays") else {err(GL_INVALID_OPERATION);return};
    for i in 0..drawcount as isize { draw(mode,*first.offset(i),*count.offset(i)); }
}

#[no_mangle]
pub unsafe extern "C" fn glMultiDrawElements(mode:u32,count:*const i32,ty:u32,indices:*const *const c_void,drawcount:i32){
    if drawcount < 0 || (drawcount > 0 && (count.is_null() || indices.is_null())) { err(GL_INVALID_VALUE); return; }
    if let Some(x)=f::<unsafe extern "C" fn(u32,*const i32,u32,*const *const c_void,i32)>("glMultiDrawElements"){x(mode,count,ty,indices,drawcount);return}
    let Some(draw)=f::<unsafe extern "C" fn(u32,i32,u32,*const c_void)>("glDrawElements") else {err(GL_INVALID_OPERATION);return};
    for i in 0..drawcount as isize { draw(mode,*count.offset(i),ty,*indices.offset(i)); }
}

#[no_mangle]
pub unsafe extern "C" fn glProvokingVertex(mode:u32){
    // ES3 has a fixed provoking-vertex convention. Do not pretend that a
    // requested LAST_VERTEX convention was applied.
    if mode != 0x8E4E /* FIRST_VERTEX_CONVENTION */ { err(GL_INVALID_OPERATION); }
}
#[no_mangle]
pub unsafe extern "C" fn glClampColor(target:u32,clamp:u32){let _=(target,clamp);/* desktop-only; ES3 is always clamped according to format */}
#[no_mangle]
pub unsafe extern "C" fn glGetFragDataIndex(_program:u32,_name:*const c_char)->i32{err(GL_INVALID_OPERATION);-1}
#[no_mangle]
pub unsafe extern "C" fn glPatchParameteri(_pname:u32,_value:i32){err(GL_INVALID_OPERATION)}
#[no_mangle]
pub unsafe extern "C" fn glMinSampleShading(_value:f32){err(GL_INVALID_OPERATION)}

#[no_mangle]
pub unsafe extern "C" fn glGetBufferParameteri64v(target:u32,pname:u32,params:*mut i64){
    if let Some(x)=f::<unsafe extern "C" fn(u32,u32,*mut i64)>("glGetBufferParameteri64v"){x(target,pname,params)}else if !params.is_null(){*params=0;err(GL_INVALID_OPERATION)}
}

#[no_mangle]
pub unsafe extern "C" fn glGetUniformIndices(program:u32,uniformCount:i32,uniformNames:*const *const c_char,uniformIndices:*mut u32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,i32,*const *const c_char,*mut u32)>("glGetUniformIndices"){x(program,uniformCount,uniformNames,uniformIndices)}else{err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glGetActiveUniformsiv(program:u32,uniformCount:i32,uniformIndices:*const u32,pname:u32,params:*mut i32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,i32,*const u32,u32,*mut i32)>("glGetActiveUniformsiv"){x(program,uniformCount,uniformIndices,pname,params)}else{err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glGetActiveUniformName(program:u32,uniformIndex:u32,bufSize:i32,length:*mut i32,name:*mut c_char){
    if let Some(x)=f::<unsafe extern "C" fn(u32,u32,i32,*mut i32,*mut c_char)>("glGetActiveUniformName"){x(program,uniformIndex,bufSize,length,name)}else{err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glGetUniformBlockIndex(program:u32,uniformBlockName:*const c_char)->u32{
    if let Some(x)=f::<unsafe extern "C" fn(u32,*const c_char)->u32>("glGetUniformBlockIndex"){x(program,uniformBlockName)}else{0xFFFF_FFFF}
}
#[no_mangle]
pub unsafe extern "C" fn glGetActiveUniformBlockiv(program:u32,uniformBlockIndex:u32,pname:u32,params:*mut i32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,u32,u32,*mut i32)>("glGetActiveUniformBlockiv"){x(program,uniformBlockIndex,pname,params)}else{err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glGetActiveUniformBlockName(program:u32,uniformBlockIndex:u32,bufSize:i32,length:*mut i32,name:*mut c_char){
    if let Some(x)=f::<unsafe extern "C" fn(u32,u32,i32,*mut i32,*mut c_char)>("glGetActiveUniformBlockName"){x(program,uniformBlockIndex,bufSize,length,name)}else{err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glUniformBlockBinding(program:u32,uniformBlockIndex:u32,uniformBlockBinding:u32){
    if let Some(x)=f::<unsafe extern "C" fn(u32,u32,u32)>("glUniformBlockBinding"){x(program,uniformBlockIndex,uniformBlockBinding)}else{err(GL_INVALID_OPERATION)}
}

pub fn resolve(name: &[u8]) -> *const c_void {
    macro_rules! r { ($($n:literal => $f:ident),* $(,)?) => { match name { $( $n => $f as *const c_void, )* _ => ptr::null(), } } }
    r!(
        b"glBindFragDataLocation"=>glBindFragDataLocation,
        b"glBindSampler"=>glBindSampler,
        b"glGenSamplers"=>glGenSamplers,
        b"glDeleteSamplers"=>glDeleteSamplers,
        b"glIsSampler"=>glIsSampler,
        b"glSamplerParameteri"=>glSamplerParameteri,
        b"glSamplerParameterf"=>glSamplerParameterf,
        b"glSamplerParameteriv"=>glSamplerParameteriv,
        b"glSamplerParameterfv"=>glSamplerParameterfv,
        b"glGetSamplerParameteriv"=>glGetSamplerParameteriv,
        b"glGetSamplerParameterfv"=>glGetSamplerParameterfv,
        b"glVertexAttribDivisor"=>glVertexAttribDivisor,
        b"glDrawArraysInstanced"=>glDrawArraysInstanced,
        b"glDrawElementsInstanced"=>glDrawElementsInstanced,
        b"glVertexAttribLPointer"=>glVertexAttribLPointer,
        b"glGetVertexAttribLdv"=>glGetVertexAttribLdv,
        b"glVertexAttribP1uiv"=>glVertexAttribP1uiv,
        b"glVertexAttribP2uiv"=>glVertexAttribP2uiv,
        b"glVertexAttribP3uiv"=>glVertexAttribP3uiv,
        b"glVertexAttribP4uiv"=>glVertexAttribP4uiv,
        b"glClearBufferiv"=>glClearBufferiv,
        b"glClearBufferuiv"=>glClearBufferuiv,
        b"glClearBufferfv"=>glClearBufferfv,
        b"glClearBufferfi"=>glClearBufferfi,
        b"glFramebufferTextureLayer"=>glFramebufferTextureLayer,
        b"glBlitFramebuffer"=>glBlitFramebuffer,
        b"glRenderbufferStorageMultisample"=>glRenderbufferStorageMultisample,
        b"glTexStorage1D"=>glTexStorage1D,
        b"glTexStorage2D"=>glTexStorage2D,
        b"glTexStorage3D"=>glTexStorage3D,
        b"glGetInteger64v"=>glGetInteger64v,
        b"glGetIntegeri_v"=>glGetIntegeri_v,
        b"glGetInteger64i_v"=>glGetInteger64i_v,
        b"glGenQueries"=>glGenQueries,
        b"glDeleteQueries"=>glDeleteQueries,
        b"glIsQuery"=>glIsQuery,
        b"glBeginQuery"=>glBeginQuery,
        b"glEndQuery"=>glEndQuery,
        b"glQueryCounter"=>glQueryCounter,
        b"glGetQueryiv"=>glGetQueryiv,
        b"glGetQueryObjectiv"=>glGetQueryObjectiv,
        b"glGetQueryObjectuiv"=>glGetQueryObjectuiv,
        b"glGetQueryObjecti64v"=>glGetQueryObjecti64v,
        b"glGetQueryObjectui64v"=>glGetQueryObjectui64v,
        b"glFenceSync"=>glFenceSync,
        b"glDeleteSync"=>glDeleteSync,
        b"glIsSync"=>glIsSync,
        b"glClientWaitSync"=>glClientWaitSync,
        b"glWaitSync"=>glWaitSync,
        b"glGetSynciv"=>glGetSynciv,
        b"glBindBufferBase"=>glBindBufferBase,
        b"glBindBufferRange"=>glBindBufferRange,
        b"glDrawRangeElements"=>glDrawRangeElements,
        b"glMultiDrawArrays"=>glMultiDrawArrays,
        b"glMultiDrawElements"=>glMultiDrawElements,
        b"glProvokingVertex"=>glProvokingVertex,
        b"glClampColor"=>glClampColor,
        b"glGetFragDataIndex"=>glGetFragDataIndex,
        b"glPatchParameteri"=>glPatchParameteri,
        b"glMinSampleShading"=>glMinSampleShading,
        b"glGetBufferParameteri64v"=>glGetBufferParameteri64v,
        b"glGetUniformIndices"=>glGetUniformIndices,
        b"glGetActiveUniformsiv"=>glGetActiveUniformsiv,
        b"glGetActiveUniformName"=>glGetActiveUniformName,
        b"glGetUniformBlockIndex"=>glGetUniformBlockIndex,
        b"glGetActiveUniformBlockiv"=>glGetActiveUniformBlockiv,
        b"glGetActiveUniformBlockName"=>glGetActiveUniformBlockName,
        b"glUniformBlockBinding"=>glUniformBlockBinding,
    )
}
