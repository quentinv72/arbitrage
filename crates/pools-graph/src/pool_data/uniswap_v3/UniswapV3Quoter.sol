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

    function pancakeV3SwapCallback(
        int256 amount0Delta,
        int256 amount1Delta,
        bytes calldata _data
    ) external {
        if (amount0Delta == 0 || amount1Delta == 0) {
            uint amountOut = 0;
            assembly {
                let memPtr := mload(0x40)
                mstore(memPtr, amountOut)
                revert(memPtr, 0x20)
            }
        }

        if (amount0Delta < 0) {
            uint amountOut = uint(-amount0Delta);
            assembly {
                let memPtr := mload(0x40)
                mstore(memPtr, amountOut)
                revert(memPtr, 0x20)
            }
        }

        if (amount1Delta < 0) {
            uint amountOut = uint(-amount1Delta);
            assembly {
                let memPtr := mload(0x40)
                mstore(memPtr, amountOut)
                revert(memPtr, 0x20)
            }
        }
    }

    function uniswapV3SwapCallback(
        int256 amount0Delta,
        int256 amount1Delta,
        bytes calldata _data
    ) external {
        if (amount0Delta == 0 || amount1Delta == 0) {
            uint amountOut = 0;
            assembly {
                let memPtr := mload(0x40)
                mstore(memPtr, amountOut)
                revert(memPtr, 0x20)
            }
        }

        if (amount0Delta < 0) {
            uint amountOut = uint(-amount0Delta);
            assembly {
                let memPtr := mload(0x40)
                mstore(memPtr, amountOut)
                revert(memPtr, 0x20)
            }
        }

        if (amount1Delta < 0) {
            uint amountOut = uint(-amount1Delta);
            assembly {
                let memPtr := mload(0x40)
                mstore(memPtr, amountOut)
                revert(memPtr, 0x20)
            }
        }
    }

    function getAmountOut(
        address pool,
        bool zeroForOne,
        uint256 amountIn
    ) external returns (uint){
        uint160 sqrtPriceLimitX96 = (
            zeroForOne
                ? 4295128749
                : 1461446703485210103287273052203988822378723970341
        );

        IUniV3Pool(pool).swap(
            address(1),
            zeroForOne,
            int256(amountIn),
            sqrtPriceLimitX96,
            ""
        );
    }
}