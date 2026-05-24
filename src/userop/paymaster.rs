use alloy::primitives::{Address, Bytes};

#[derive(Clone, Debug)]
pub struct PaymasterConfig {
    pub address: Address,
    pub verification_gas_limit: u128,
    pub post_op_gas_limit: u128,
}

impl PaymasterConfig {
    pub fn for_compass(address: Address) -> Self {
        Self {
            address,
            verification_gas_limit: 60_000,
            post_op_gas_limit: 50_000,
        }
    }

    pub fn pack(&self) -> Bytes {
        let mut out = Vec::with_capacity(52);
        out.extend_from_slice(self.address.as_slice());
        out.extend_from_slice(&self.verification_gas_limit.to_be_bytes());
        out.extend_from_slice(&self.post_op_gas_limit.to_be_bytes());
        Bytes::from(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::primitives::address;

    #[test]
    fn pack_layout_matches_v07_spec() {
        let pm = PaymasterConfig {
            address: address!("0000000000000000000000000000000000000abc"),
            verification_gas_limit: 100_000,
            post_op_gas_limit: 200_000,
        };
        let bytes = pm.pack();
        assert_eq!(bytes.len(), 52);
        assert_eq!(&bytes[..20], pm.address.as_slice());
        let mut vbuf = [0u8; 16];
        vbuf.copy_from_slice(&bytes[20..36]);
        assert_eq!(u128::from_be_bytes(vbuf), 100_000);
        let mut pbuf = [0u8; 16];
        pbuf.copy_from_slice(&bytes[36..52]);
        assert_eq!(u128::from_be_bytes(pbuf), 200_000);
    }
}
