use alloy::sol;

sol! {
    #[sol(rpc)]
    interface IGatewayWallet {
        function deposit(address token, uint256 amount) external;
        function withdraw(address token, uint256 amount) external;
        function depositedBalance(address depositor, address token) external view returns (uint256);
        function addDelegate(address token, address delegate) external;
        function removeDelegate(address token, address delegate) external;
    }

    #[sol(rpc)]
    interface IGatewayMinter {
        function gatewayMint(bytes calldata attestation, bytes calldata signature) external;
    }
}
