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

use format_translate;

const GL_INVALID_ENUM: u32 = 0x0500;
const GL_INVALID_VALUE: u32 = 0x0501;
const GL_INVALID_OPERATION: u32 = 0x0502;
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_TEXTURE_WIDTH: u32 = 0x1000;
const GL_TEXTURE_HEIGHT: u32 = 0x1001;
const GL_ACTIVE_TEXTURE: u32 = 0x84E0;
const GL_TEXTURE0: u32 = 0x84C0;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_SAMPLES_PASSED: u32 = 0x8914;
const GL_TIME_ELAPSED: u32 = 0x88BF;
const GL_QUERY_COUNTER_BITS: u32 = 0x8864;
const GL_SYNC_GPU_COMMANDS_COMPLETE: u32 = 0x9117;
const GL_WAIT_FAILED: u32 = 0x911D;

fn err(e: u32) { crate::errors().set(e); }

unsafe fn f<T: Copy>(name: &'static str) -> Option<T> { crate::driver_fn_cached::<T>(name) }

unsafe fn call_void1(name: &'static str, a: u32) -> bool {
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

unsafe fn call_void2(name: &'static str, a: u32, b: u32) -> bool {
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

// GL 3.2 ranged draw with a base vertex. ES 3.2 defines this directly, so use it when the
// driver has it. Otherwise the cases separate cleanly: with `base_vertex == 0` the call is
// exactly `glDrawElements` at a byte offset of `start * stride`. A base vertex is a *vertex*
// offset, not a byte offset, so it cannot be folded into that offset -- doing so would read
// entirely different indices. That case needs the index data rewritten, so it is reported
// rather than mis-rendered.
#[no_mangle]
pub unsafe extern "C" fn glDrawRangeElementsBaseVertex(
    mode: u32,
    start: u32,
    end: u32,
    count: i32,
    ty: u32,
    indices: *const c_void,
    base_vertex: i32,
) {
    if count <= 0 {
        return;
    }
    if let Some(direct) = f::<unsafe extern "C" fn(u32, u32, u32, i32, u32, *const c_void, i32)>(
        "glDrawRangeElementsBaseVertex",
    ) {
        return direct(mode, start, end, count, ty, indices, base_vertex);
    }
    if let Some(direct) = f::<unsafe extern "C" fn(u32, i32, u32, *const c_void, i32)>(
        "glDrawElementsBaseVertex",
    ) {
        return direct(mode, count, ty, indices, base_vertex);
    }
    if base_vertex == 0 {
        if let Some(draw) = f::<unsafe extern "C" fn(u32, i32, u32, *const c_void)>("glDrawElements") {
            return draw(mode, count, ty, indices);
        }
    }
    static ONCE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    crate::log_once(
        &ONCE,
        "[gl33] glDrawRangeElementsBaseVertex needs a GLES base-vertex entry point on this driver",
    );
    err(GL_INVALID_OPERATION);
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
    let ifmt = format_translate::map_storage_internal(internalformat, crate::render_caps());
    if let Some(x)=f::<unsafe extern "C" fn(u32,i32,u32,i32,i32)>("glRenderbufferStorageMultisample"){x(target,samples,ifmt,w,h)}else{err(GL_INVALID_OPERATION)}
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
    let internalformat = format_translate::map_storage_internal(internalformat, crate::render_caps());
    if let Some(x)=f::<unsafe extern "C" fn(u32,i32,u32,i32,i32)>("glTexStorage2D"){x(target,levels,internalformat,width,height)}else{err(GL_INVALID_OPERATION)}
}
#[no_mangle]
pub unsafe extern "C" fn glTexStorage3D(target:u32,levels:i32,internalformat:u32,width:i32,height:i32,depth:i32){
    let internalformat = format_translate::map_storage_internal(internalformat, crate::render_caps());
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

// ---- Multi-bind (GL 3.1 / ARB_multi_bind) ----------------------------------------------------
//
// The unit bindings are emulated by selecting the unit, binding, and restoring the previously
// active unit. `glBindTextureUnit` binds a bare texture name with no target, and desktop GL
// resolves that per target; 2D is the only target that can be reached without extra state, so
// 1D/3D/cube textures must still be bound with glActiveTexture + glBindTexture.

/// Saves the active texture unit and 2D binding, restoring both on drop.
struct TextureBindingGuard {
    unit: i32,
    texture: i32,
}

impl TextureBindingGuard {
    /// # Safety
    /// Queries the driver, so a context must be current.
    unsafe fn capture() -> Option<Self> {
        let get = f::<unsafe extern "C" fn(u32, *mut i32)>("glGetIntegerv")?;
        let mut unit = 0i32;
        let mut texture = 0i32;
        get(GL_ACTIVE_TEXTURE, &mut unit);
        get(0x8069 /* GL_TEXTURE_BINDING_2D */, &mut texture);
        Some(Self { unit, texture })
    }

    /// # Safety
    /// Writes driver state, so a context must be current.
    unsafe fn restore(self) {
        if let Some(x) = f::<unsafe extern "C" fn(u32)>("glActiveTexture") {
            // GL_ACTIVE_TEXTURE is an enum offset from GL_TEXTURE0; a 0 here means the
            // query failed, and passing 0 to glActiveTexture is a GL_INVALID_ENUM.
            x(if self.unit >= GL_TEXTURE0 as i32 { self.unit as u32 } else { GL_TEXTURE0 });
        }
        if let Some(x) = f::<unsafe extern "C" fn(u32, u32)>("glBindTexture") {
            x(GL_TEXTURE_2D, self.texture as u32);
        }
    }
}

/// `GL_MAX_COMBINED_TEXTURE_IMAGE_UNITS`, measured once. Desktop GL keeps the per-unit and
/// combined limits apart; the combined limit is the one that bounds `unit` here.
/// # Safety
/// Queries the driver, so a context must be current.
unsafe fn max_texture_units() -> u32 {
    static MAX: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let cached = MAX.load(std::sync::atomic::Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }
    let mut n = 0i32;
    if let Some(get) = f::<unsafe extern "C" fn(u32, *mut i32)>("glGetIntegerv") {
        get(0x8B4D /* GL_MAX_COMBINED_TEXTURE_IMAGE_UNITS */, &mut n);
    }
    let n = if n > 0 { n as u32 } else { 32 };
    MAX.store(n, std::sync::atomic::Ordering::Relaxed);
    n
}

#[no_mangle]
pub unsafe extern "C" fn glBindTextureUnit(unit: u32, texture: u32) {
    if unit >= max_texture_units() {
        err(GL_INVALID_VALUE);
        return;
    }
    let active = f::<unsafe extern "C" fn(u32)>("glActiveTexture");
    let bind = f::<unsafe extern "C" fn(u32, u32)>("glBindTexture");
    let (Some(active), Some(bind)) = (active, bind) else {
        err(GL_INVALID_OPERATION);
        return;
    };
    let mut prev = 0i32;
    if let Some(get) = f::<unsafe extern "C" fn(u32, *mut i32)>("glGetIntegerv") {
        get(GL_ACTIVE_TEXTURE, &mut prev);
    }
    active(GL_TEXTURE0 + unit);
    bind(GL_TEXTURE_2D, texture);
    // Same rule as TextureBindingGuard::restore: a failed query leaves prev at 0, which is
    // not a valid glActiveTexture argument.
    active(if prev >= GL_TEXTURE0 as i32 { prev as u32 } else { GL_TEXTURE0 });
}

// ---- Direct-state-access texture getters (GL 3.0/3.1) ---------------------------------------
//
// These take a texture *name* instead of a target, and GLES 3.x has no DSA. Each one binds
// the texture to the active unit, calls the bound-target entry point, and puts the previous
// binding back. The signatures follow LWJGL, which omits the `bufSize` argument of
// glGetTextureImage.

#[no_mangle]
pub unsafe extern "C" fn glGetTextureLevelParameteriv(texture: u32, level: i32, pname: u32, params: *mut i32) {
    if params.is_null() {
        err(GL_INVALID_VALUE);
        return;
    }
    let Some(guard) = TextureBindingGuard::capture() else {
        err(GL_INVALID_OPERATION);
        return;
    };
    if let Some(x) = f::<unsafe extern "C" fn(u32, u32)>("glBindTexture") {
        x(GL_TEXTURE_2D, texture);
    }
    if let Some(x) = f::<unsafe extern "C" fn(u32, i32, u32, *mut i32)>("glGetTexLevelParameteriv") {
        x(GL_TEXTURE_2D, level, pname, params);
    } else if !params.is_null() {
        *params = 0;
        err(GL_INVALID_OPERATION);
    }
    guard.restore();
}

#[no_mangle]
pub unsafe extern "C" fn glGetTextureParameteriv(texture: u32, pname: u32, params: *mut i32) {
    if params.is_null() {
        err(GL_INVALID_VALUE);
        return;
    }
    let Some(guard) = TextureBindingGuard::capture() else {
        err(GL_INVALID_OPERATION);
        return;
    };
    if let Some(x) = f::<unsafe extern "C" fn(u32, u32)>("glBindTexture") {
        x(GL_TEXTURE_2D, texture);
    }
    if let Some(x) = f::<unsafe extern "C" fn(u32, u32, *mut i32)>("glGetTexParameteriv") {
        x(GL_TEXTURE_2D, pname, params);
    } else if !params.is_null() {
        *params = 0;
        err(GL_INVALID_OPERATION);
    }
    guard.restore();
}

#[no_mangle]
pub unsafe extern "C" fn glGetTextureParameterfv(texture: u32, pname: u32, params: *mut f32) {
    if params.is_null() {
        err(GL_INVALID_VALUE);
        return;
    }
    let Some(guard) = TextureBindingGuard::capture() else {
        err(GL_INVALID_OPERATION);
        return;
    };
    if let Some(x) = f::<unsafe extern "C" fn(u32, u32)>("glBindTexture") {
        x(GL_TEXTURE_2D, texture);
    }
    if let Some(x) = f::<unsafe extern "C" fn(u32, u32, *mut f32)>("glGetTexParameterfv") {
        x(GL_TEXTURE_2D, pname, params);
    } else if !params.is_null() {
        *params = 0.0;
        err(GL_INVALID_OPERATION);
    }
    guard.restore();
}

/// 2D level read-back. BGRA/BGR are read through RGBA/RGB and swizzled on the way out,
/// because GLES 3.x cannot read those formats directly. Targets with more than one layer
/// (3D, cube, 2D array) are forwarded unchanged so the driver's own error surfaces.
#[no_mangle]
pub unsafe extern "C" fn glGetTexImage(target: u32, level: i32, format: u32, ty: u32, pixels: *mut c_void) {
    let components = if format_translate::is_bgra8(format, ty) {
        Some((format_translate::GL_RGBA, 4usize))
    } else if format_translate::is_bgr8(format, ty) {
        Some((format_translate::GL_RGB, 3usize))
    } else {
        None
    };
    let Some((native_format, comps)) = components else {
        forward_get_tex_image(target, level, format, ty, pixels);
        return;
    };
    if target != GL_TEXTURE_2D || pixels.is_null() {
        // Size query (pixels == null) or a target we do not translate here.
        forward_get_tex_image(target, level, format, ty, pixels);
        return;
    }
    let Some(get_level) = f::<unsafe extern "C" fn(u32, i32, u32, *mut i32)>("glGetTexLevelParameteriv")
    else {
        err(GL_INVALID_OPERATION);
        return;
    };
    let (mut w, mut h) = (0i32, 0i32);
    get_level(target, level, GL_TEXTURE_WIDTH, &mut w);
    get_level(target, level, GL_TEXTURE_HEIGHT, &mut h);
    if w <= 0 || h <= 0 {
        err(GL_INVALID_OPERATION);
        return;
    }
    let n = w as usize * h as usize;
    let mut tmp = vec![0u8; n * comps];
    if let Some(x) = f::<unsafe extern "C" fn(u32, i32, u32, u32, *mut c_void)>("glGetTexImage") {
        x(target, level, native_format, GL_UNSIGNED_BYTE, tmp.as_mut_ptr() as *mut c_void);
    } else {
        err(GL_INVALID_OPERATION);
        return;
    }
    let out = std::slice::from_raw_parts_mut(pixels as *mut u8, n * comps);
    if comps == 4 {
        out.copy_from_slice(&format_translate::swizzle_bgra_to_rgba(&tmp, n));
    } else {
        out.copy_from_slice(&format_translate::swizzle_bgr_to_rgb(&tmp, n));
    }
}

unsafe fn forward_get_tex_image(target: u32, level: i32, format: u32, ty: u32, pixels: *mut c_void) {
    if let Some(x) = f::<unsafe extern "C" fn(u32, i32, u32, u32, *mut c_void)>("glGetTexImage") {
        x(target, level, format, ty, pixels);
    } else {
        err(GL_INVALID_OPERATION);
    }
}

#[no_mangle]
pub unsafe extern "C" fn glGetTextureImage(
    texture: u32,
    level: i32,
    format: u32,
    ty: u32,
    buf_size: i32,
    pixels: *mut c_void,
) {
    let _ = buf_size;
    let Some(guard) = TextureBindingGuard::capture() else {
        err(GL_INVALID_OPERATION);
        return;
    };
    if let Some(x) = f::<unsafe extern "C" fn(u32, u32)>("glBindTexture") {
        x(GL_TEXTURE_2D, texture);
    }
    glGetTexImage(GL_TEXTURE_2D, level, format, ty, pixels);
    guard.restore();
}

// ---- Program interface queries (GL 3.3 / ARB_program_interface_query) -----------------------
// Present in GLES 3.1. On a 3.0 driver these are genuinely absent.

#[no_mangle]
pub unsafe extern "C" fn glGetProgramInterfaceiv(program: u32, iface: u32, pname: u32, params: *mut i32) {
    if let Some(x) = f::<unsafe extern "C" fn(u32, u32, u32, *mut i32)>("glGetProgramInterfaceiv") {
        x(program, iface, pname, params);
    } else {
        if !params.is_null() {
            *params = 0;
        }
        err(GL_INVALID_OPERATION);
    }
}
#[no_mangle]
pub unsafe extern "C" fn glGetProgramStageiv(program: u32, stage: u32, pname: u32, params: *mut i32) {
    if let Some(x) = f::<unsafe extern "C" fn(u32, u32, u32, *mut i32)>("glGetProgramStageiv") {
        x(program, stage, pname, params);
    } else {
        if !params.is_null() {
            *params = 0;
        }
        err(GL_INVALID_OPERATION);
    }
}
#[no_mangle]
pub unsafe extern "C" fn glGetProgramResourceIndex(program: u32, iface: u32, name: *const c_char) -> u32 {
    if let Some(x) = f::<unsafe extern "C" fn(u32, u32, *const c_char) -> u32>("glGetProgramResourceIndex") {
        x(program, iface, name)
    } else {
        err(GL_INVALID_OPERATION);
        0xFFFF_FFFF
    }
}
#[no_mangle]
pub unsafe extern "C" fn glGetProgramResourceiv(program: u32, iface: u32, index: u32, count: i32, props: *const u32, count_len: i32, length: *mut i32, params: *mut i32) {
    if let Some(x) = f::<unsafe extern "C" fn(u32, u32, u32, i32, *const u32, i32, *mut i32, *mut i32)>("glGetProgramResourceiv") {
        x(program, iface, index, count, props, count_len, length, params);
    } else {
        err(GL_INVALID_OPERATION);
    }
}
#[no_mangle]
pub unsafe extern "C" fn glGetProgramResourceName(program: u32, iface: u32, index: u32, buf_size: i32, length: *mut i32, name: *mut c_char) {
    if let Some(x) = f::<unsafe extern "C" fn(u32, u32, u32, i32, *mut i32, *mut c_char)>("glGetProgramResourceName") {
        x(program, iface, index, buf_size, length, name);
    } else {
        err(GL_INVALID_OPERATION);
    }
}

// ---- GL 3.1 calls with no GLES 3.x equivalent ------------------------------------------------
// GL_TEXTURE_1D and the texel-buffer targets were never adopted into ES. Rejecting keeps a
// caller that probes-and-falls-back working instead of corrupting bindings.

#[no_mangle]
pub unsafe extern "C" fn glTexSubImage1D(_t: u32, _l: i32, _x: i32, _w: i32, _f: u32, _ty: u32, _data: *const c_void) {
    err(GL_INVALID_ENUM);
}
#[no_mangle]
pub unsafe extern "C" fn glTexBuffer(_t: u32, _r: u32, _b: u32) {
    err(GL_INVALID_ENUM);
}
#[no_mangle]
pub unsafe extern "C" fn glTexBufferRange(_t: u32, _r: u32, _b: u32, _o: isize, _s: isize) {
    err(GL_INVALID_ENUM);
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
        b"glBindTextureUnit"=>glBindTextureUnit,
        b"glGetTextureLevelParameteriv"=>glGetTextureLevelParameteriv,
        b"glGetTextureParameteriv"=>glGetTextureParameteriv,
        b"glGetTextureParameterfv"=>glGetTextureParameterfv,
        b"glGetTexImage"=>glGetTexImage,
        b"glGetTextureImage"=>glGetTextureImage,
        b"glGetProgramInterfaceiv"=>glGetProgramInterfaceiv,
        b"glGetProgramStageiv"=>glGetProgramStageiv,
        b"glGetProgramResourceIndex"=>glGetProgramResourceIndex,
        b"glGetProgramResourceiv"=>glGetProgramResourceiv,
        b"glGetProgramResourceName"=>glGetProgramResourceName,
        b"glTexSubImage1D"=>glTexSubImage1D,
        b"glTexBuffer"=>glTexBuffer,
        b"glTexBufferRange"=>glTexBufferRange,
    )
}

// The GL 3.0 vector forms of glVertexAttrib. A measured pass against the GL 3.0 core list
// showed these four were the only ones never wired up; ES 3.x implements all of them.
crate::gl_passthrough!(
    glVertexAttrib1fv(index: u32, v: *const f32);
    glVertexAttrib2fv(index: u32, v: *const f32);
    glVertexAttrib3fv(index: u32, v: *const f32);
    glVertexAttrib4fv(index: u32, v: *const f32);
);
