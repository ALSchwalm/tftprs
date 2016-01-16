#[derive(Debug)]
pub enum TransferMode {
    NetAscii,
    Octet,
    // 'email' is unsupported
}

#[derive(Debug)]
pub enum Opcode {
    ReadRequest,
    WriteRequest,
    Data,
    Acknowledgment,
    Error,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ErrorCode {
    Undefined = 0,
    FileNotFound,
    AccessViolation,
    DiskFull,
    IllegalOperation,
    UnknownTransferID,
    FileExists,
    NoSuchUser,

    // Internal error codes
    SilentError,
}
