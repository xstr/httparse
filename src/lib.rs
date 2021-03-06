#![feature(core)]
#![allow(unused_assignments)]

use std::str;


macro_rules! eof {
    ($buf:expr, $i:expr) => (
        if $buf.len() == $i {
            return Ok(Status::Partial);
        }
    )
}
macro_rules! next {
    ($buf:expr, $i:ident) => ({
        let buf = $buf;
        eof!(buf, $i);
        let ret = unsafe { *$buf.get_unchecked($i) };
        $i += 1;
        ret
    })
}

macro_rules! expect {
    ($buf:ident[$i:ident] == $pat:pat => $ret:expr) => {
        match next!($buf, $i) {
            v@$pat => v,
            _ => return $ret
        }
    }
}

macro_rules! slice {
    ($buf:ident[$from:expr;]) => ({
        slice!($buf[$from; $buf.len()])
    });
    ($buf:ident[$from:expr;$to:expr]) => {
        unsafe {
            use std::mem::transmute;
            use std::raw::Slice;
            transmute(Slice {
                data: $buf.as_ptr().offset($from as isize),
                len: $to - $from
            })
        }
    }
}

macro_rules! shrink {
    ($slice:ident, $to:expr) => ({
        unsafe {
            use std::mem::transmute;
            use std::raw::Slice;
            use $crate::Header;
            let raw: &mut &mut Slice<Header> = transmute(&$slice);
            raw.len = $to;
        }
    })
}

/// Determines if byte is a token char.
///
/// > ```notrust
/// > token          = 1*tchar
/// >
/// > tchar          = "!" / "#" / "$" / "%" / "&" / "'" / "*"
/// >                / "+" / "-" / "." / "^" / "_" / "`" / "|" / "~"
/// >                / DIGIT / ALPHA
/// >                ; any VCHAR, except delimiters
/// > ```
macro_rules! is_token {
    ($b:expr) => ({
        let b = $b;
        b > 0x1F && b < 0x7F
    })
}

macro_rules! parse {
    ($obj:ident.$field:ident = parse_version ($buf:expr)) => ({
        $obj.$field = match try!(parse_version($buf)) {
            Status::Complete(val) => {
                let buf = $buf;
                $buf = slice!(buf[8;]);
                Some(val)
            },
            Status::Partial => return Ok(Status::Partial)
        }
    });
    ($obj:ident.$field:ident = parse_code ($buf:expr)) => ({
        $obj.$field = match try!(parse_code($buf)) {
            Status::Complete(val) => {
                let buf = $buf;
                $buf = slice!(buf[3;]);
                Some(val)
            },
            Status::Partial => return Ok(Status::Partial)
        }
    });
    ($obj:ident.$field:ident = $action:ident ($buf:expr)) => ({
        $obj.$field = match try!($action($buf)) {
            Status::Complete(val) => {
                let buf = $buf;
                $buf = slice!(buf[val.len();]);
                Some(val)
            },
            Status::Partial => return Ok(Status::Partial)
        }
    })
}

macro_rules! newline {
    ($buf:ident) => ({
        let mut i = 0;
        eof!($buf, i);
        match next!($buf, i) {
            b'\r' => {
                expect!($buf[i] == b'\n' => Ok(Status::Partial));
                let buf = $buf;
                $buf = slice!(buf[2;]);
            },
            b'\n' => {
                let buf = $buf;
                $buf = slice!(buf[1;]);
            },
            _ => return Err(Error::NewLine)
        }
    })
}


#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Error {
    HeaderName,
    HeaderValue,
    NewLine,
    Status,
    Token,
    TooManyHeaders,
    Version
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub enum Status<T> {
    Complete(T),
    Partial
}

impl<T> Status<T> {
    #[inline]
    pub fn is_complete(&self) -> bool {
        match *self {
            Status::Complete(..) => true,
            Status::Partial => false
        }
    }

    #[inline]
    pub fn is_partial(&self) -> bool {
        match *self {
            Status::Complete(..) => false,
            Status::Partial => true
        }
    }

    #[inline]
    pub fn unwrap(self) -> T {
        match self {
            Status::Complete(t) => t,
            Status::Partial => panic!("Tried to unwrap Status::Partial")
        }
    }
}

pub struct Request<'a> {
    pub method: Option<&'a str>,
    pub path: Option<&'a str>,
    pub version: Option<u8>,
    pub headers: &'a mut [Header<'a>]
}

impl<'a> Request<'a> {
    #[inline]
    pub fn new(headers: &'a mut [Header<'a>]) -> Request<'a> {
        Request {
            method: None,
            path: None,
            version: None,
            headers: headers,
        }
    }

    pub fn parse(&mut self, mut buf: &'a [u8]) -> Result<Status<usize>, Error> {
        let orig_len = buf.len();
        parse!(self.method = parse_token(buf));
        buf = slice!(buf[1;]);
        parse!(self.path = parse_token(buf));
        buf = slice!(buf[1;]);
        parse!(self.version = parse_version(buf));
        newline!(buf);

        let len = orig_len - buf.len();
        let headers_len = match try!(parse_headers(&mut self.headers, buf)) {
            Status::Complete(len) => len,
            Status::Partial => return Ok(Status::Partial)
        };

        Ok(Status::Complete(len + headers_len))
    }
}

pub struct Response<'a> {
    pub version: Option<u8>,
    pub code: Option<u16>,
    pub reason: Option<&'a str>,
    pub headers: &'a mut [Header<'a>]
}

impl<'a> Response<'a> {
    #[inline]
    pub fn new(headers: &'a mut [Header<'a>]) -> Response<'a> {
        Response {
            version: None,
            code: None,
            reason: None,
            headers: headers,
        }
    }

    pub fn parse(&mut self, mut buf: &'a [u8]) -> Result<Status<usize>, Error> {
        let orig_len = buf.len();

        parse!(self.version = parse_version(buf));
        buf = slice!(buf[1;]);
        parse!(self.code = parse_code(buf));
        buf = slice!(buf[1;]);
        parse!(self.reason = parse_token(buf));
        newline!(buf);

        let len = orig_len - buf.len();
        let headers_len = match try!(parse_headers(&mut self.headers, buf)) {
            Status::Complete(len) => len,
            Status::Partial => return Ok(Status::Partial)
        };
        Ok(Status::Complete(len + headers_len))
    }
}

#[derive(Copy, Clone, PartialEq, Debug)]
pub struct Header<'a> {
    pub name: &'a str,
    pub value: &'a [u8],
}

pub const EMPTY_HEADER: Header<'static> = Header { name: "", value: b"" };

#[inline]
fn parse_version(buf: &[u8]) -> Result<Status<u8>, Error> {
    let mut i = 0;

    expect!(buf[i] == b'H' => Err(Error::Version));
    expect!(buf[i] == b'T' => Err(Error::Version));
    expect!(buf[i] == b'T' => Err(Error::Version));
    expect!(buf[i] == b'P' => Err(Error::Version));
    expect!(buf[i] == b'/' => Err(Error::Version));
    expect!(buf[i] == b'1' => Err(Error::Version));
    expect!(buf[i] == b'.' => Err(Error::Version));
    match next!(buf, i) {
        b'0' => Ok(Status::Complete(0)),
        b'1' => Ok(Status::Complete(1)),
        _ => Err(Error::Version)
    }
}

#[inline]
fn parse_token(buf: &[u8]) -> Result<Status<&str>, Error> {
    let mut i: usize = 0;
    loop {
        let b = next!(buf, i);
        if b == b' ' || b == b'\r' || b == b'\n' {
            return Ok(Status::Complete(unsafe {
                // all bytes up till `i` must have been `is_token`.
                str::from_utf8_unchecked(&buf[..i - 1])
            }));
        } else if !is_token!(b) {
            return Err(Error::Token);
        }
    }
}

#[inline]
fn parse_code(buf: &[u8]) -> Result<Status<u16>, Error> {
    let mut i = 0;
    let hundreds = expect!(buf[i] == b'0'...b'9' => Err(Error::Status));
    let tens = expect!(buf[i] == b'0'...b'9' => Err(Error::Status));
    let ones = expect!(buf[i] == b'0'...b'9' => Err(Error::Status));

    Ok(Status::Complete((hundreds - b'0') as u16 * 100 +
                        (tens - b'0') as u16 * 10 +
                        (ones - b'0') as u16))
}

#[inline]
fn parse_headers<'a>(headers: &mut &mut [Header<'a>], buf: &'a [u8]) -> Result<Status<usize>, Error> {
    let mut i: usize = 0;
    let mut num_headers: usize = 0;
    let mut last_i: usize = 0;
    let mut result = Err(Error::TooManyHeaders);

    {
        let mut iter = headers.iter_mut();

        'headers: loop {
            // a newline here means the head is over!
            eof!(buf, i);
            let b = unsafe { *buf.get_unchecked(i) };
            if b == b'\r' {
                i += 1;
                expect!(buf[i] == b'\n' => Err(Error::NewLine));
                result = Ok(Status::Complete(i));
                break;
            } else if b == b'\n' {
                i += 1;
                result = Ok(Status::Complete(i));
                break;
            }

            let header = match iter.next() {
                Some(header) => header,
                None => break 'headers
            };

            num_headers += 1;
            last_i = i;
            // parse header name until colon
            loop {
                let b = next!(buf, i);
                if b == b':' {
                    header.name = unsafe {
                        str::from_utf8_unchecked(slice!(buf[last_i;i - 1]))
                    };
                    break;
                } else if !is_token!(b) {
                    println!("{:?} {:?}", b, b as char);
                    return Err(Error::HeaderName);
                }
            }

            // eat white space between colon and value
            loop {
                let b = next!(buf, i);
                if !(b == b' ' || b == b'\t') {
                    i -= 1;
                    last_i = i;
                    break;
                }
            }

            // parse value till EOL

            macro_rules! check {
                () => ({
                    let b = unsafe { *buf.get_unchecked(i) };
                    i += 1;
                    if !is_token!(b) {
                        if (b < 0o40 && b != 0o11) || b == 0o177 {
                            if b == b'\r' {
                                expect!(buf[i] == b'\n' => Err(Error::HeaderValue));
                                header.value = slice!(buf[last_i;i - 2]);
                                continue 'headers;
                            } else if b == b'\n' {
                                header.value = slice!(buf[last_i;i - 1]);
                                continue 'headers;
                            } else {
                                return Err(Error::HeaderValue);
                            }
                        }
                    }
                })
            }
            while buf.len() - i >= 8 {
                check!();
                check!();
                check!();
                check!();
                check!();
                check!();
                check!();
                check!();
            }
            loop {
                let b = next!(buf, i);
                if !is_token!(b) {
                    if (b < 0o40 && b != 0o11) || b == 0o177 {
                        if b == b'\r' {
                            expect!(buf[i] == b'\n' => Err(Error::HeaderValue));
                            header.value = slice!(buf[last_i;i - 2]);
                            break;
                        } else if b == b'\n' {
                            header.value = slice!(buf[last_i;i - 1]);
                            break;
                        } else {
                            return Err(Error::HeaderValue);
                        }
                    }
                }
            }
        }
    } // drop iter

    shrink!(headers, num_headers);
    result
}

#[cfg(test)]
mod tests {
    use super::{Request, Response, Status, EMPTY_HEADER};

    const NUM_OF_HEADERS: usize = 4;

    #[test]
    fn test_shrink() {
        let mut arr = [EMPTY_HEADER; 16];
        {
            let slice = &mut &mut arr[..];
            assert_eq!(slice.len(), 16);
            shrink!(slice, 4);
            assert_eq!(slice.len(), 4);
        }
        assert_eq!(arr.len(), 16);
    }

    macro_rules! req {
        ($name:ident, $buf:expr, $closure:expr) => (
            req! {$name, $buf, Ok(Status::Complete($buf.len())), $closure }
        );
        ($name:ident, $buf:expr, $len:expr, $closure:expr) => (
        #[test]
        fn $name() {
            let mut headers = [EMPTY_HEADER; NUM_OF_HEADERS];
            let mut req = Request::new(&mut headers[..]);
            let closure: Box<Fn(Request)> = Box::new($closure);
            let status = req.parse(unsafe { ::std::mem::transmute($buf) });
            assert_eq!(status, $len);
            closure(req);
        }
        )
    }

    req! {
        test_request_simple,
        "GET / HTTP/1.1\r\n\r\n",
        |req| {
            assert_eq!(req.method.unwrap(), "GET");
            assert_eq!(req.path.unwrap(), "/");
            assert_eq!(req.version.unwrap(), 1);
            assert_eq!(req.headers.len(), 0);
        }
    }

    req! {
        test_request_headers,
        "GET / HTTP/1.1\r\nHost: foo.com\r\nCookie: \r\n\r\n",
        |req| {
            assert_eq!(req.method.unwrap(), "GET");
            assert_eq!(req.path.unwrap(), "/");
            assert_eq!(req.version.unwrap(), 1);
            assert_eq!(req.headers.len(), 2);
            assert_eq!(req.headers[0].name, "Host");
            assert_eq!(req.headers[0].value, b"foo.com");
            assert_eq!(req.headers[1].name, "Cookie");
            assert_eq!(req.headers[1].value, b"");
        }
    }

    req! {
        test_request_headers_max,
        "GET / HTTP/1.1\r\nA: A\r\nB: B\r\nC: C\r\nD: D\r\n\r\n",
        |req| {
            assert_eq!(req.headers.len(), NUM_OF_HEADERS);
        }
    }

    req! {
        test_request_multibyte,
        b"GET / HTTP/1.1\r\nHost: foo.com\r\nUser-Agent: \xe3\x81\xb2\xe3/1.0\r\n\r\n",
        |req| {
            assert_eq!(req.method.unwrap(), "GET");
            assert_eq!(req.path.unwrap(), "/");
            assert_eq!(req.version.unwrap(), 1);
            assert_eq!(req.headers[0].name, "Host");
            assert_eq!(req.headers[0].value, b"foo.com");
            assert_eq!(req.headers[1].name, "User-Agent");
            assert_eq!(req.headers[1].value, b"\xe3\x81\xb2\xe3/1.0");
        }
    }


    req! {
        test_request_partial,
        "GET / HTTP/1.1\r\n\r", Ok(Status::Partial),
        |_req| {}
    }

    req! {
        test_request_newlines,
        "GET / HTTP/1.1\n\n",
        |_| {}
    }


    macro_rules! res {
        ($name:ident, $buf:expr, $closure:expr) => (
            res! {$name, $buf, Ok(Status::Complete($buf.len())), $closure }
        );
        ($name:ident, $buf:expr, $len:expr, $closure:expr) => (
        #[test]
        fn $name() {
            let mut headers = [EMPTY_HEADER; NUM_OF_HEADERS];
            let mut res = Response::new(&mut headers[..]);
            let closure: Box<Fn(Response)> = Box::new($closure);
            let status = res.parse(unsafe { ::std::mem::transmute($buf.as_bytes()) });
            assert_eq!(status, $len);
            closure(res);
        }
        )
    }

    res! {
        test_response_simple,
        "HTTP/1.1 200 OK\r\n\r\n",
        |res| {
            assert_eq!(res.version.unwrap(), 1);
            assert_eq!(res.code.unwrap(), 200);
            assert_eq!(res.reason.unwrap(), "OK");
        }
    }
}
