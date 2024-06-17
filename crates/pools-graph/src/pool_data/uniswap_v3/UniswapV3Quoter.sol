pragma solidity ^0.8.0;

interface IUniV3Pool {
    function swap(
        address recipient,
        bool zeroForOne,
        int256 amountSpecified,
        uint160 sqrtPriceLimitX96,
        bytes calldata data
    ) external returns (int256 amount0, int256 amount1);
}

contract UniswapV3Quoter {
    receive() external payable {}

    function uniswapV3SwapCallback(
        int256 amount0Delta,
        int256 amount1Delta,
        bytes calldata _data
    ) external {
        assembly {
            memPtr := mload(0x40)
            mstore(memPtr, amount0Delta)
            mstore(add(memPtr, 0x20), amount1Delta)
            revert(memPtr, 0x40)
        }
        revert(string(abi.encode(amount0Delta, amount1Delta)));
    }

    function getAmountOut(
        address pool,
        bool zeroForOne,
        uint256 amountIn
    ) external returns (int256, int256){
        uint160 sqrtPriceLimitX96 = (
            zeroForOne
                ? 4295128749
                : 1461446703485210103287273052203988822378723970341
        );

        try IUniV3Pool(pool).swap(
            address(1),
            zeroForOne,
            int256(amountIn),
            sqrtPriceLimitX96,
            ""
        ) {} catch (bytes memory reason) {
            (int256 amount0Delta, int256 amount1Delta) = abi.decode(reason, (int256, int256));
            return (amount0Delta, amount1Delta);
        }
    }
}