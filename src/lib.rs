#![crate_type = "lib"]
#![license = "MIT/ASL2"]
#![feature(globs, unsafe_destructor, phase)]

#[phase(plugin, link)] extern crate log;

extern crate libc;

extern crate libnanomsg;

pub use result::{NanoResult, NanoError};

use libc::{c_int, c_void, size_t};
use std::mem::transmute;
use std::ptr;
use result::{SocketInitializationError, SocketBindError, SocketOptionError};
use std::io::{Writer, Reader, IoResult};
use std::io;
use std::mem::size_of;
use std::time::duration::Duration;
use endpoint::Endpoint;
use std::kinds::marker::ContravariantLifetime;

mod result;
mod endpoint;

const DEFAULT_BUF_SIZE: uint = 1024 * 64;

/// Type-safe protocols that Nanomsg uses. Each socket
/// is bound to a single protocol that has specific behaviour
/// (such as only being able to receive messages and not send 'em).
#[deriving(Show, PartialEq)]
pub enum Protocol {
    Req,
    Rep,
    Push,
    Pull,
    Pair,
    Bus,
    Pub,
    Sub,
    Surveyor,
    Respondent
}

/// A type-safe socket wrapper around nanomsg's own socket implementation. This
/// provides a safe interface for dealing with initializing the sockets, sending
/// and receiving messages.
pub struct Socket<'a> {
    socket: c_int,
    marker: ContravariantLifetime<'a>
}

impl<'a> Socket<'a> {

    /// Allocate and initialize a new Nanomsg socket which returns
    /// a new file descriptor behind the scene. The safe interface doesn't
    /// expose any of the underlying file descriptors and such.
    ///
    /// Usage:
    ///
    /// ```rust
    /// use nanomsg::{Socket, Pull};
    ///
    /// let mut socket = match Socket::new(Pull) {
    ///     Ok(socket) => socket,
    ///     Err(err) => panic!("{}", err)
    /// };
    /// ```
    pub fn new(protocol: Protocol) -> NanoResult<Socket<'a>> {

        let proto = match protocol {
            Req => libnanomsg::NN_REQ,
            Rep => libnanomsg::NN_REP,
            Push => libnanomsg::NN_PUSH,
            Pull => libnanomsg::NN_PULL,
            Pair => libnanomsg::NN_PAIR,
            Bus => libnanomsg::NN_BUS,
            Pub => libnanomsg::NN_PUB,
            Sub => libnanomsg::NN_SUB,
            Surveyor => libnanomsg::NN_SURVEYOR,
            Respondent => libnanomsg::NN_RESPONDENT
        };

        let socket = unsafe {
            libnanomsg::nn_socket(libnanomsg::AF_SP, proto)
        };

        if socket == -1 {
            return Err(NanoError::new("Failed to create a new nanomsg socket. Error: {}", SocketInitializationError));
        }

        debug!("Initialized a new raw socket");

        Ok(Socket {
            socket: socket,
            marker: ContravariantLifetime::<'a>
        })
    }

    /// Creating a new socket through `Socket::new` does **not**
    /// bind that socket to a listening state. Instead, one has to be
    /// explicit in enabling the socket to listen onto a specific address.
    ///
    /// That's what the `bind` method does. Passing in a raw string like:
    /// "ipc:///tmp/pipeline.ipc" is supported.
    ///
    /// Note: This does **not** block the current task. That job
    /// is up to the user of the library by entering a loop.
    ///
    /// Usage:
    ///
    /// ```rust
    /// use nanomsg::{Socket, Pull};
    ///
    /// let mut socket = match Socket::new(Pull) {
    ///     Ok(socket) => socket,
    ///     Err(err) => panic!("{}", err)
    /// };
    ///
    /// // Bind the newly created socket to the following address:
    /// //match socket.bind("ipc:///tmp/pipeline.ipc") {
    /// //    Ok(_) => {},
    /// //   Err(err) => panic!("Failed to bind socket: {}", err)
    /// //}
    /// ```
    pub fn bind<'b, 'a: 'b>(&mut self, addr: &str) -> NanoResult<Endpoint<'b>> {
        let ret = unsafe { libnanomsg::nn_bind(self.socket, addr.to_c_str().as_ptr() as *const i8) };

        if ret == -1 {
            return Err(NanoError::new(format!("Failed to find the socket to the address: {}", addr), SocketBindError));
        }

        Ok(Endpoint::new(ret, self.socket))
    }

    pub fn connect(&mut self, addr: &str) -> NanoResult<()> {
        let ret = unsafe { libnanomsg::nn_connect(self.socket, addr.to_c_str().as_ptr() as *const i8) };

        if ret == -1 {
            return Err(NanoError::new(format!("Failed to find the socket to the address: {}", addr), SocketBindError));
        }

        Ok(())
    }

    // --------------------------------------------------------------------- //
    // Generic socket options                                                //
    // --------------------------------------------------------------------- //

    // TODO set comments according to http://nanomsg.org/v0.4/nn_setsockopt.3.html
    
    pub fn set_linger(&mut self, linger: &Duration) -> NanoResult<()> {
        let milliseconds = linger.num_milliseconds();
        let c_linger = milliseconds as c_int;
        let c_linger_ptr = &c_linger as *const _ as *const c_void;
        let ret = unsafe { ;
            libnanomsg::nn_setsockopt (
                self.socket, 
                libnanomsg::NN_SOL_SOCKET, 
                libnanomsg::NN_LINGER, 
                c_linger_ptr, 
                size_of::<c_int>() as size_t) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to set linger to {}", linger), SocketOptionError));
        }

        Ok(())
    }

    pub fn set_send_buffer_size(&mut self, size_in_bytes: int) -> NanoResult<()> {
        let c_size_in_bytes = size_in_bytes as c_int;
        let c_size_ptr = &c_size_in_bytes as *const _ as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (
                self.socket, 
                libnanomsg::NN_SOL_SOCKET, 
                libnanomsg::NN_SNDBUF, 
                c_size_ptr, 
                size_of::<c_int>() as size_t) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to set send buffer size to {}", size_in_bytes), SocketOptionError));
        }

        Ok(())
    }

    pub fn set_receive_buffer_size(&mut self, size_in_bytes: int) -> NanoResult<()> {
        let c_size_in_bytes = size_in_bytes as c_int;
        let c_size_ptr = &c_size_in_bytes as *const _ as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (
                self.socket, 
                libnanomsg::NN_SOL_SOCKET, 
                libnanomsg::NN_RCVBUF, 
                c_size_ptr, 
                size_of::<c_int>() as size_t) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to set receive buffer size to {}", size_in_bytes), SocketOptionError));
        }

        Ok(())
    }

    pub fn set_send_timeout(&mut self, timeout: &Duration) -> NanoResult<()> {
        let milliseconds = timeout.num_milliseconds();
        let c_timeout = milliseconds as c_int;
        let c_timeout_ptr = &c_timeout as *const _ as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (
                self.socket, 
                libnanomsg::NN_SOL_SOCKET, 
                libnanomsg::NN_SNDTIMEO, 
                c_timeout_ptr, 
                size_of::<c_int>() as size_t) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to set send timeout to {}", timeout), SocketOptionError));
        }

        Ok(())
    }

    pub fn set_receive_timeout(&mut self, timeout: &Duration) -> NanoResult<()> {
        let milliseconds = timeout.num_milliseconds();
        let c_timeout = milliseconds as c_int;
        let c_timeout_ptr = &c_timeout as *const _ as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (
                self.socket, 
                libnanomsg::NN_SOL_SOCKET, 
                libnanomsg::NN_RCVTIMEO, 
                c_timeout_ptr, 
                size_of::<c_int>() as size_t) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to set receive timeout to {}", timeout), SocketOptionError));
        }

        Ok(())
    }

    pub fn set_reconnect_interval(&mut self, interval: &Duration) -> NanoResult<()> {
        let milliseconds = interval.num_milliseconds();
        let c_interval = milliseconds as c_int;
        let c_interval_ptr = &c_interval as *const _ as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (
                self.socket, 
                libnanomsg::NN_SOL_SOCKET, 
                libnanomsg::NN_RECONNECT_IVL, 
                c_interval_ptr, 
                size_of::<c_int>() as size_t) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to set reconnect interval to {}", interval), SocketOptionError));
        }

        Ok(())
    }

    pub fn set_max_reconnect_interval(&mut self, interval: &Duration) -> NanoResult<()> {
        let milliseconds = interval.num_milliseconds();
        let c_interval = milliseconds as c_int;
        let c_interval_ptr = &c_interval as *const _ as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (
                self.socket, 
                libnanomsg::NN_SOL_SOCKET, 
                libnanomsg::NN_RECONNECT_IVL_MAX, 
                c_interval_ptr, 
                size_of::<c_int>() as size_t) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to set max reconnect interval to {}", interval), SocketOptionError));
        }

        Ok(())
    }

    pub fn set_send_priority(&mut self, priority: u8) -> NanoResult<()> {
        let c_priority = priority as c_int;
        let c_priority_ptr = &c_priority as *const _ as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (
                self.socket, 
                libnanomsg::NN_SOL_SOCKET, 
                libnanomsg::NN_SNDPRIO, 
                c_priority_ptr, 
                size_of::<c_int>() as size_t) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to set send priority to {}", priority), SocketOptionError));
        }

        Ok(())
    }

    pub fn set_receive_priority(&mut self, priority: u8) -> NanoResult<()> {
        let c_priority = priority as c_int;
        let c_priority_ptr = &c_priority as *const _ as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (
                self.socket, 
                libnanomsg::NN_SOL_SOCKET, 
                libnanomsg::NN_RCVPRIO, 
                c_priority_ptr, 
                size_of::<c_int>() as size_t) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to set receive priority to {}", priority), SocketOptionError));
        }

        Ok(())
    }

    pub fn set_ipv4_only(&mut self, ipv4_only: bool) -> NanoResult<()> {
        let c_ipv4_only = if ipv4_only { 1 as c_int } else { 0 as c_int };
        let option_value_ptr = &c_ipv4_only as *const _ as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (
                self.socket, 
                libnanomsg::NN_SOL_SOCKET, 
                libnanomsg::NN_IPV4ONLY, 
                option_value_ptr, 
                size_of::<c_int>() as size_t) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to set ipv4 only to {}", ipv4_only), SocketOptionError));
        }

        Ok(())
    }
    
    pub fn set_socket_name(&mut self, name: &str) -> NanoResult<()> {
        let name_len = name.len() as size_t;
        let name_c_str = name.to_c_str();
        let name_ptr = name_c_str.as_ptr();
        let name_raw_ptr = name_ptr as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (
                self.socket, 
                libnanomsg::NN_SOL_SOCKET, 
                libnanomsg::NN_SOCKET_NAME, 
                name_raw_ptr, 
                name_len) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to set the socket name to: {}", name), SocketOptionError));
        }

        Ok(())
    }

    // --------------------------------------------------------------------- //
    // TCP transport socket options                                          //
    // --------------------------------------------------------------------- //
    pub fn set_tcp_nodelay(&mut self, tcp_nodelay: bool) -> NanoResult<()> {
        let c_tcp_nodelay = if tcp_nodelay { 1 as c_int } else { 0 as c_int };
        let option_value_ptr = &c_tcp_nodelay as *const _ as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (
                self.socket, 
                libnanomsg::NN_TCP, 
                libnanomsg::NN_TCP_NODELAY, 
                option_value_ptr, 
                size_of::<c_int>() as size_t) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to set tcp nodelay to {}", tcp_nodelay), SocketOptionError));
        }

        Ok(())
    }

    // --------------------------------------------------------------------- //
    // PubSub protocol socket options                                        //
    // --------------------------------------------------------------------- //
    pub fn subscribe(&mut self, topic: &str) -> NanoResult<()> {
        let topic_len = topic.len() as size_t;
        let topic_c_str = topic.to_c_str();
        let topic_ptr = topic_c_str.as_ptr();
        let topic_raw_ptr = topic_ptr as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (self.socket, libnanomsg::NN_SUB, libnanomsg::NN_SUB_SUBSCRIBE, topic_raw_ptr, topic_len) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to subscribe to the topic: {}", topic), SocketOptionError));
        }

        Ok(())
    }

    pub fn unsubscribe(&mut self, topic: &str) -> NanoResult<()> {
        let topic_len = topic.len() as size_t;
        let topic_c_str = topic.to_c_str();
        let topic_ptr = topic_c_str.as_ptr();
        let topic_raw_ptr = topic_ptr as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (self.socket, libnanomsg::NN_SUB, libnanomsg::NN_SUB_UNSUBSCRIBE, topic_raw_ptr, topic_len) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to unsubscribe from the topic: {}", topic), SocketOptionError));
        }

        Ok(())
    }

    // --------------------------------------------------------------------- //
    // Survey protocol socket options                                        //
    // --------------------------------------------------------------------- //

    pub fn set_survey_deadline(&mut self, deadline: &Duration) -> NanoResult<()> {
        let milliseconds = deadline.num_milliseconds();
        let c_deadline = milliseconds as c_int;
        let c_deadline_ptr = &c_deadline as *const _ as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (
                self.socket, 
                libnanomsg::NN_SURVEYOR, 
                libnanomsg::NN_SURVEYOR_DEADLINE, 
                c_deadline_ptr, 
                size_of::<c_int>() as size_t) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to set survey deadline to {}", deadline), SocketOptionError));
        }

        Ok(())
    }

    // --------------------------------------------------------------------- //
    // Request/reply protocol socket options                                        //
    // --------------------------------------------------------------------- //

    pub fn set_request_resend_interval(&mut self, interval: &Duration) -> NanoResult<()> {
        let milliseconds = interval.num_milliseconds();
        let c_interval = milliseconds as c_int;
        let c_interval_ptr = &c_interval as *const _ as *const c_void;
        let ret = unsafe { 
            libnanomsg::nn_setsockopt (
                self.socket, 
                libnanomsg::NN_REQ, 
                libnanomsg::NN_REQ_RESEND_IVL, 
                c_interval_ptr, 
                size_of::<c_int>() as size_t) 
        };
 
        if ret == -1 {
            return Err(NanoError::new(format!("Failed to set request resend interval to {}", interval), SocketOptionError));
        }

        Ok(())
    }

}

impl<'a> Reader for Socket<'a> {
    fn read(&mut self, buf: &mut [u8]) -> IoResult<uint> {
        let mut mem : *mut u8 = ptr::null_mut();

        let ret = unsafe {
            libnanomsg::nn_recv(self.socket, transmute(&mut mem),
                libnanomsg::NN_MSG, 0 as c_int)
        };

        if ret == -1 {
            return Err(io::standard_error(io::OtherIoError));
        }

        unsafe { ptr::copy_memory(buf.as_mut_ptr(), mem as *const u8, buf.len() as uint) };

        unsafe { libnanomsg::nn_freemsg(mem as *mut c_void) };

        Ok(ret as uint)
    }

    fn read_to_end(&mut self) -> IoResult<Vec<u8>> {
        let mut buf = Vec::with_capacity(DEFAULT_BUF_SIZE);
        match self.push_at_least(1, DEFAULT_BUF_SIZE, &mut buf) {
            Ok(_) => {}
            Err(e) => return Err(e)
        }
        return Ok(buf);
    }
}

impl<'a> Writer for Socket<'a> {
    fn write(&mut self, buf: &[u8]) -> IoResult<()> {
        let len = buf.len();
        let ret = unsafe {
            libnanomsg::nn_send(self.socket, buf.as_ptr() as *const c_void,
                                len as size_t, 0)
        };

        if ret as uint != len {
            return Err(io::standard_error(io::OtherIoError));
        }

        Ok(())
    }
}

#[unsafe_destructor]
impl<'a> Drop for Socket<'a> {
    fn drop(&mut self) {
        unsafe { libnanomsg::nn_close(self.socket); }
    }
}

#[cfg(test)]
mod tests {
    #![allow(unused_must_use)]
    #[phase(plugin, link)]
    extern crate log;
    extern crate libnanomsg;
    extern crate libc;

    use super::*;

    use std::time::duration::Duration;
    use std::io::timer::sleep;

    #[test]
    fn initialize_socket() {
        let socket = match Socket::new(Pull) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        assert!(socket.socket >= 0);

        drop(socket)
    }

    #[test]
    fn bind_socket() {
        let mut socket = match Socket::new(Pull) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        match socket.bind("ipc:///tmp/bind_socket.ipc") {
            Ok(_) => {},
            Err(err) => panic!("{}", err)
        }

        drop(socket)
    }

    #[test]
    fn receive_from_socket() {
        spawn(proc() {
            let mut socket = match Socket::new(Pull) {
                Ok(socket) => socket,
                Err(err) => panic!("{}", err)
            };


            match socket.bind("ipc:///tmp/pipeline.ipc") {
                Ok(_) => {},
                Err(err) => panic!("{}", err)
            }

            let mut buf = [0u8, ..6];
            match socket.read(&mut buf) {
                Ok(len) => {
                    assert_eq!(len, 6);
                    assert_eq!(buf.as_slice(), b"foobar")
                },
                Err(err) => panic!("{}", err)
            }

            drop(socket)
        });

        let mut socket = match Socket::new(Push) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        match socket.connect("ipc:///tmp/pipeline.ipc") {
            Ok(_) => {},
            Err(err) => panic!("{}", err)
        }

        match socket.write(b"foobar") {
            Ok(..) => {},
            Err(err) => panic!("Failed to write to the socket: {}", err)
        }
 
        drop(socket)
   }


    #[test]
    fn receive_string_from_req_rep_socket() {
        spawn(proc() {
            let mut socket = match Socket::new(Rep) {
                Ok(socket) => socket,
                Err(err) => panic!("{}", err)
            };


            match socket.bind("ipc:///tmp/reqrep.ipc") {
                Ok(_) => {},
                Err(err) => panic!("{}", err)
            }

            match socket.read_to_string() {
                Ok(message) => {
                    assert_eq!(message.as_slice(), "This is a long string for the test.")
                },
                Err(err) => panic!("{}", err)
            }

            drop(socket)
        });

        let mut socket = match Socket::new(Req) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        match socket.connect("ipc:///tmp/reqrep.ipc") {
            Ok(_) => {},
            Err(err) => panic!("{}", err)
        }

        match socket.write_str("This is a long string for the test.") {
            Ok(..) => {},
            Err(err) => panic!("Failed to write to the socket: {}", err)
        }
 
        drop(socket)
   }


    #[test]
    fn send_and_recv_from_socket_in_pair() {
        spawn(proc() {
            let mut socket = match Socket::new(Pair) {
                Ok(socket) => socket,
                Err(err) => panic!("{}", err)
            };


            match socket.bind("ipc:///tmp/pair.ipc") {
                Ok(_) => {},
                Err(err) => panic!("{}", err)
            }

            let mut buf = [0u8, ..6];
            match socket.read(&mut buf) {
                Ok(len) => {
                    assert_eq!(len, 6);
                    assert_eq!(buf.as_slice(), b"foobar")
                },
                Err(err) => panic!("{}", err)
            }

            match socket.write(b"foobaz") {
                Ok(..) => {},
                Err(err) => panic!("Failed to write to the socket: {}", err)
            }

            drop(socket)
        });

        let mut socket = match Socket::new(Pair) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        match socket.connect("ipc:///tmp/pair.ipc") {
            Ok(_) => {},
            Err(err) => panic!("{}", err)
        }

        match socket.write(b"foobar") {
            Ok(..) => {},
            Err(err) => panic!("Failed to write to the socket: {}", err)
        }

        let mut buf = [0u8, ..6];
        match socket.read(&mut buf) {
            Ok(len) => {
                assert_eq!(len, 6);
                assert_eq!(buf.as_slice(), b"foobaz")
            },
            Err(err) => panic!("{}", err)
        }
 
        drop(socket)
    }

    #[test]
    fn send_and_receive_from_socket_in_bus() {
        
        spawn(proc() {
            let mut socket = match Socket::new(Bus) {
                Ok(socket) => socket,
                Err(err) => panic!("{}", err)
            };


            match socket.connect("ipc:///tmp/bus.ipc") {
                Ok(_) => {},
                Err(err) => panic!("{}", err)
            }

            let mut buf = [0u8, ..6];
            match socket.read(&mut buf) {
                Ok(len) => {
                    assert_eq!(len, 6);
                    assert_eq!(buf.as_slice(), b"foobar")
                },
                Err(err) => panic!("{}", err)
            }

            drop(socket)
        });
        
        spawn(proc() {
            let mut socket = match Socket::new(Bus) {
                Ok(socket) => socket,
                Err(err) => panic!("{}", err)
            };


            match socket.connect("ipc:///tmp/bus.ipc") {
                Ok(_) => {},
                Err(err) => panic!("{}", err)
            }

            let mut buf = [0u8, ..6];
            match socket.read(&mut buf) {
                Ok(len) => {
                    assert_eq!(len, 6);
                    assert_eq!(buf.as_slice(), b"foobar")
                },
                Err(err) => panic!("{}", err)
            }

            drop(socket)
        });

        let mut socket = match Socket::new(Bus) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        match socket.bind("ipc:///tmp/bus.ipc") {
            Ok(_) => {},
            Err(err) => panic!("{}", err)
        }

        sleep(Duration::milliseconds(200));

        match socket.write(b"foobar") {
            Ok(..) => {},
            Err(err) => panic!("Failed to write to the socket: {}", err)
        }

        drop(socket)
    }

    #[test]
    fn send_and_receive_from_socket_in_pubsub() {
        
        spawn(proc() {
            let mut socket = match Socket::new(Sub) {
                Ok(socket) => socket,
                Err(err) => panic!("{}", err)
            };

            match socket.subscribe("foo") {
                Ok(_) => {},
                Err(err) => panic!("{}", err)
            }

            match socket.connect("ipc:///tmp/pubsub.ipc") {
                Ok(_) => {},
                Err(err) => panic!("{}", err)
            }

            let mut buf = [0u8, ..6];
            match socket.read(&mut buf) {
                Ok(len) => {
                    assert_eq!(len, 6);
                    assert_eq!(buf.as_slice(), b"foobar")
                },
                Err(err) => panic!("{}", err)
            }

            drop(socket)
        });
        
        spawn(proc() {
            let mut socket = match Socket::new(Sub) {
                Ok(socket) => socket,
                Err(err) => panic!("{}", err)
            };

            match socket.subscribe("foo") {
                Ok(_) => {},
                Err(err) => panic!("{}", err)
            }

            match socket.connect("ipc:///tmp/pubsub.ipc") {
                Ok(_) => {},
                Err(err) => panic!("{}", err)
            }

            let mut buf = [0u8, ..6];
            match socket.read(&mut buf) {
                Ok(len) => {
                    assert_eq!(len, 6);
                    assert_eq!(buf.as_slice(), b"foobar")
                },
                Err(err) => panic!("{}", err)
            }

            drop(socket)
        });

        let mut socket = match Socket::new(Pub) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        match socket.bind("ipc:///tmp/pubsub.ipc") {
            Ok(_) => {},
            Err(err) => panic!("{}", err)
        }

        sleep(Duration::milliseconds(200));

        match socket.write(b"foobar") {
            Ok(..) => {},
            Err(err) => panic!("Failed to write to the socket: {}", err)
        }

        drop(socket)
    }

    #[test]
    fn send_and_receive_from_socket_in_survey() {
        
        spawn(proc() {
            let mut socket = match Socket::new(Respondent) {
                Ok(socket) => socket,
                Err(err) => panic!("{}", err)
            };

            match socket.connect("ipc:///tmp/survey.ipc") {
                Ok(_) => {},
                Err(err) => panic!("{}", err)
            }

            let mut buf = [0u8, ..9];
            match socket.read(&mut buf) {
                Ok(len) => {
                    assert_eq!(len, 9);
                    assert_eq!(buf.as_slice(), b"yes_or_no")
                },
                Err(err) => panic!("{}", err)
            }

            match socket.write(b"yes") {
                Ok(..) => {},
                Err(err) => panic!("Failed to write to the socket: {}", err)
            }

            drop(socket)
        });
        
        spawn(proc() {
            let mut socket = match Socket::new(Respondent) {
                Ok(socket) => socket,
                Err(err) => panic!("{}", err)
            };

            match socket.connect("ipc:///tmp/survey.ipc") {
                Ok(_) => {},
                Err(err) => panic!("{}", err)
            }

            let mut buf = [0u8, ..9];
            match socket.read(&mut buf) {
                Ok(len) => {
                    assert_eq!(len, 9);
                    assert_eq!(buf.as_slice(), b"yes_or_no")
                },
                Err(err) => panic!("{}", err)
            }

            match socket.write(b"YES") {
                Ok(..) => {},
                Err(err) => panic!("Failed to write to the socket: {}", err)
            }

            drop(socket)
        });

        let mut socket = match Socket::new(Surveyor) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        let deadline = Duration::milliseconds(500);
        match socket.set_survey_deadline(&deadline) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        match socket.bind("ipc:///tmp/survey.ipc") {
            Ok(_) => {},
            Err(err) => panic!("{}", err)
        }

        sleep(Duration::milliseconds(200));

        match socket.write(b"yes_or_no") {
            Ok(..) => {},
            Err(err) => panic!("Failed to write to the socket: {}", err)
        }

        let mut buf = [0u8, ..3];
        match socket.read(&mut buf) {
            Ok(len) => {
                assert_eq!(len, 3);
                assert!(buf.as_slice() == b"yes" || buf.as_slice() == b"YES")
            },
            Err(err) => panic!("{}", err)
        }

        match socket.read(&mut buf) {
            Ok(len) => {
                assert_eq!(len, 3);
                assert!(buf.as_slice() == b"yes" || buf.as_slice() == b"YES")
            },
            Err(err) => panic!("{}", err)
        }
        
        drop(socket)
    }

    #[test]
    fn should_change_linger() {

        let mut socket = match Socket::new(Pair) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        assert!(socket.socket >= 0);

        let linger = Duration::milliseconds(1024);
        match socket.set_linger(&linger) {
            Ok(..) => {},
            Err(err) => panic!("Failed to change linger on the socket: {}", err)
        }

        drop(socket)
    }

    #[test]
    fn should_change_send_buffer_size() {

        let mut socket = match Socket::new(Pair) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        assert!(socket.socket >= 0);

        let size: int = 64 * 1024;
        match socket.set_send_buffer_size(size) {
            Ok(..) => {},
            Err(err) => panic!("Failed to change send buffer size on the socket: {}", err)
        }

        drop(socket)
    }

    #[test]
    fn should_change_receive_buffer_size() {

        let mut socket = match Socket::new(Pair) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        assert!(socket.socket >= 0);

        let size: int = 64 * 1024;
        match socket.set_receive_buffer_size(size) {
            Ok(..) => {},
            Err(err) => panic!("Failed to change receive buffer size on the socket: {}", err)
        }

        drop(socket)
    }

    #[test]
    fn should_change_send_timeout() {

        let mut socket = match Socket::new(Pair) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        assert!(socket.socket >= 0);

        let timeout = Duration::milliseconds(-2);
        match socket.set_send_timeout(&timeout) {
            Ok(..) => {},
            Err(err) => panic!("Failed to change send timeout on the socket: {}", err)
        }

        drop(socket)
    }

    #[test]
    fn should_change_receive_timeout() {

        let mut socket = match Socket::new(Pair) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        assert!(socket.socket >= 0);

        let timeout = Duration::milliseconds(200);
        match socket.set_receive_timeout(&timeout) {
            Ok(..) => {},
            Err(err) => panic!("Failed to change receive timeout on the socket: {}", err)
        }

        drop(socket)
    }

    #[test]
    fn should_change_reconnect_interval() {

        let mut socket = match Socket::new(Pair) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        assert!(socket.socket >= 0);

        let interval = Duration::milliseconds(142);
        match socket.set_reconnect_interval(&interval) {
            Ok(..) => {},
            Err(err) => panic!("Failed to change reconnect interval on the socket: {}", err)
        }

        drop(socket)
    }

    #[test]
    fn should_change_max_reconnect_interval() {

        let mut socket = match Socket::new(Pair) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        assert!(socket.socket >= 0);

        let interval = Duration::milliseconds(666);
        match socket.set_max_reconnect_interval(&interval) {
            Ok(..) => {},
            Err(err) => panic!("Failed to change reconnect interval on the socket: {}", err)
        }

        drop(socket)
    }

    #[test]
    fn should_change_send_priority() {

        let mut socket = match Socket::new(Pair) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        assert!(socket.socket >= 0);

        match socket.set_send_priority(15u8) {
            Ok(..) => {},
            Err(err) => panic!("Failed to change send priority on the socket: {}", err)
        }

        drop(socket)
    }

    #[test]
    fn should_change_receive_priority() {

        let mut socket = match Socket::new(Pair) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        assert!(socket.socket >= 0);

        match socket.set_receive_priority(2u8) {
            Ok(..) => {},
            Err(err) => panic!("Failed to change receive priority on the socket: {}", err)
        }

        drop(socket)
    }

    #[test]
    fn should_change_ipv4_only() {

        let mut socket = match Socket::new(Pair) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        assert!(socket.socket >= 0);

        match socket.set_ipv4_only(true) {
            Ok(..) => {},
            Err(err) => panic!("Failed to change ipv4 only on the socket: {}", err)
        }

        drop(socket)
    }

    #[test]
    fn should_change_socket_name() {

        let mut socket = match Socket::new(Pair) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        assert!(socket.socket >= 0);

        match socket.set_socket_name("bob") {
            Ok(..) => {},
            Err(err) => panic!("Failed to change the socket name: {}", err)
        }

        drop(socket)
    }


    #[test]
    fn should_change_request_resend_interval() {

        let mut socket = match Socket::new(Req) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        assert!(socket.socket >= 0);

        let interval = Duration::milliseconds(60042);
        match socket.set_request_resend_interval(&interval) {
            Ok(..) => {},
            Err(err) => panic!("Failed to change request resend interval on the socket: {}", err)
        }

        drop(socket)
    }

    #[test]
    fn should_change_tcp_nodelay() {

        let mut socket = match Socket::new(Pair) {
            Ok(socket) => socket,
            Err(err) => panic!("{}", err)
        };

        assert!(socket.socket >= 0);

        match socket.set_tcp_nodelay(true) {
            Ok(..) => {},
            Err(err) => panic!("Failed to change tcp nodelay only on the socket: {}", err)
        }

        drop(socket)
    }
}
