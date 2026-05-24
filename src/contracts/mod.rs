use alloy::sol;

sol! {
    struct PackedUserOperation {
        address sender;
        uint256 nonce;
        bytes initCode;
        bytes callData;
        bytes32 accountGasLimits;     // verificationGasLimit << 128 | callGasLimit
        uint256 preVerificationGas;
        bytes32 gasFees;              // maxPriorityFeePerGas << 128 | maxFeePerGas
        bytes paymasterAndData;
        bytes signature;
    }

    #[sol(rpc)]
    interface IEntryPoint {
        function handleOps(PackedUserOperation[] calldata ops, address payable beneficiary) external;
        function getNonce(address sender, uint192 key) external view returns (uint256);
        function getUserOpHash(PackedUserOperation calldata userOp) external view returns (bytes32);
        function depositTo(address account) external payable;
        function balanceOf(address account) external view returns (uint256);

        event UserOperationEvent(
            bytes32 indexed userOpHash,
            address indexed sender,
            address indexed paymaster,
            uint256 nonce,
            bool success,
            uint256 actualGasCost,
            uint256 actualGasUsed
        );

        event UserOperationRevertReason(
            bytes32 indexed userOpHash,
            address indexed sender,
            uint256 nonce,
            bytes revertReason
        );
    }

    struct InitArgs {
        address entryPoint;
        address usdc;
        address gatewayWallet;
        address gatewayMinter;
        address aavePool;
        address upgradeAuthority;
        address paymaster;
    }

    #[sol(rpc)]
    interface ICompassAccountFactory {
        function createAccount(
            address owner,
            uint256 salt,
            InitArgs calldata initArgs
        ) external returns (address account);

        function getAccountAddress(address owner, uint256 salt) external view returns (address);
        function totalAccounts() external view returns (uint256);
        function accountAt(uint256 i) external view returns (address);
        function isFactoryAccount(address account) external view returns (bool);
        function accountsRange(uint256 from, uint256 to) external view returns (address[] memory);
    }

    #[sol(rpc)]
    interface ISecurityFacet {
        function registerSession(
            address agent,
            uint64 expiresAt,
            bytes4[] calldata allowedSelectors
        ) external;
        function revokeSession(address agent) external;
        function isSessionValid(address agent, bytes4 selector) external view returns (bool);
        function sessionExpiry(address agent) external view returns (uint64);
    }

    #[sol(rpc)]
    interface IGatewayFacet {
        function depositToGateway(uint256 amount) external;
        function withdrawFromGateway(uint256 amount) external;
        function gatewayBalance() external view returns (uint256);
    }

    #[sol(rpc)]
    interface IAaveFacet {
        function supplyAave(uint256 amount) external;
        function withdrawAave(uint256 amount) external;
    }

    #[sol(rpc)]
    interface IAccount4337Facet {
        function execute(address target, uint256 value, bytes calldata data) external;
        function executeBatch(
            address[] calldata targets,
            uint256[] calldata values,
            bytes[] calldata datas
        ) external;
    }

    struct FacetCut {
        address facetAddress;
        uint8 action;                  // 0=Add, 1=Replace, 2=Remove
        bytes4[] functionSelectors;
    }

    #[sol(rpc)]
    interface IProtocolUpgradeFacet {
        function authorityAddFacet(FacetCut[] calldata cuts) external;
        function userRevokeUpgradeAuthority() external;
        function currentAuthority() external view returns (address);
        function isAuthorityRevoked() external view returns (bool);
    }

    #[sol(rpc)]
    interface IOwnable {
        function owner() external view returns (address);
    }
}
