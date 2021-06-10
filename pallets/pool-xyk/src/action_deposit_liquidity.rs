// This file is part of the SORA network and Polkaswap app.

// Copyright (c) 2020, 2021, Polka Biome Ltd. All rights reserved.
// SPDX-License-Identifier: BSD-4-Clause

// Redistribution and use in source and binary forms, with or without modification,
// are permitted provided that the following conditions are met:

// Redistributions of source code must retain the above copyright notice, this list
// of conditions and the following disclaimer.
// Redistributions in binary form must reproduce the above copyright notice, this
// list of conditions and the following disclaimer in the documentation and/or other
// materials provided with the distribution.
//
// All advertising materials mentioning features or use of this software must display
// the following acknowledgement: This product includes software developed by Polka Biome
// Ltd., SORA, and Polkaswap.
//
// Neither the name of the Polka Biome Ltd. nor the names of its contributors may be used
// to endorse or promote products derived from this software without specific prior written permission.

// THIS SOFTWARE IS PROVIDED BY Polka Biome Ltd. AS IS AND ANY EXPRESS OR IMPLIED WARRANTIES,
// INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR
// A PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL Polka Biome Ltd. BE LIABLE FOR ANY
// DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING,
// BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS;
// OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT,
// STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
// USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use frame_support::dispatch::DispatchResult;
use frame_support::weights::Weight;
use frame_support::{dispatch, ensure};

use common::prelude::{Balance, FixedWrapper};

use crate::{to_balance, to_fixed_wrapper};

use crate::aliases::{AccountIdOf, AssetIdOf, TechAccountIdOf, TechAssetIdOf};
use crate::{Config, Error, Pallet, MIN_LIQUIDITY};

use crate::bounds::*;
use crate::operations::*;

impl<T: Config> common::SwapRulesValidation<AccountIdOf<T>, TechAccountIdOf<T>, T>
    for DepositLiquidityAction<
        AssetIdOf<T>,
        TechAssetIdOf<T>,
        Balance,
        AccountIdOf<T>,
        TechAccountIdOf<T>,
    >
{
    fn is_abstract_checking(&self) -> bool {
        (self.source.0).amount == Bounds::Dummy
            || (self.source.1).amount == Bounds::Dummy
            || self.destination.amount == Bounds::Dummy
    }

    fn prepare_and_validate(&mut self, source_opt: Option<&AccountIdOf<T>>) -> DispatchResult {
        let abstract_checking = source_opt.is_none() || common::SwapRulesValidation::<AccountIdOf<T>, TechAccountIdOf<T>, T>::is_abstract_checking(self);

        // Check that client account is same as source, because signature is checked for source.
        // Signature checking is used in extrinsics for example, and source is derived from origin.
        // TODO: In general case it is possible to use different client account, for example if
        // signature of source is legal for some source accounts.
        if !abstract_checking {
            let source = source_opt.unwrap();
            match &self.client_account {
                // Just use `client_account` as copy of source.
                None => {
                    self.client_account = Some(source.clone());
                }
                Some(ca) => {
                    if ca != source {
                        Err(Error::<T>::SourceAndClientAccountDoNotMatchAsEqual)?;
                    }
                }
            }

            // Dealing with receiver account, for example case then not swapping to self, but to
            // other account.
            match &self.receiver_account {
                // Just use `client_account` as same account, swapping to self.
                None => {
                    self.receiver_account = self.client_account.clone();
                }
                _ => (),
            }
        }

        let pool_account_repr_sys =
            technical::Pallet::<T>::tech_account_id_to_account_id(&self.pool_account)?;
        // Check that pool account is valid.
        Pallet::<T>::is_pool_account_valid_for(self.source.0.asset, &self.pool_account)?;

        let mark_asset = Pallet::<T>::get_marking_asset(&self.pool_account)?;
        ensure!(
            self.destination.asset == mark_asset,
            Error::<T>::InvalidAssetForLiquidityMarking
        );

        let repr_k_asset_id = self.destination.asset.into();

        // Balance of source account for asset pair.
        let (balance_bs, balance_ts) = if abstract_checking {
            (None, None)
        } else {
            let source = source_opt.unwrap();
            (
                Some(<assets::Pallet<T>>::free_balance(
                    &self.source.0.asset,
                    &source,
                )?),
                Some(<assets::Pallet<T>>::free_balance(
                    &self.source.1.asset,
                    &source,
                )?),
            )
        };

        if !abstract_checking && (balance_bs.unwrap() <= 0 || balance_ts.unwrap() <= 0) {
            Err(Error::<T>::AccountBalanceIsInvalid)?;
        }

        // Balance of pool account for asset pair basic asset.
        let balance_bp =
            <assets::Pallet<T>>::free_balance(&self.source.0.asset, &pool_account_repr_sys)?;
        // Balance of pool account for asset pair target asset.
        let balance_tp =
            <assets::Pallet<T>>::free_balance(&self.source.1.asset, &pool_account_repr_sys)?;

        let mut empty_pool = false;
        if balance_bp == 0 && balance_tp == 0 {
            empty_pool = true;
        } else if balance_bp <= 0 {
            Err(Error::<T>::PoolIsInvalid)?;
        } else if balance_tp <= 0 {
            Err(Error::<T>::PoolIsInvalid)?;
        }

        #[allow(unused_variables)]
        let mut init_x = 0;
        #[allow(unused_variables)]
        let mut init_y = 0;
        if !abstract_checking && empty_pool {
            // Convertation from `Bounds` to `Option` is used here, and it is posible that value
            // None value returned from conversion.
            init_x = Option::<Balance>::from((self.source.0).amount)
                .ok_or(Error::<T>::InitialLiqudityDepositRatioMustBeDefined)?;
            init_y = Option::<Balance>::from((self.source.1).amount)
                .ok_or(Error::<T>::InitialLiqudityDepositRatioMustBeDefined)?;
        }

        // FixedWrapper version of variables.
        let fxw_balance_bp = FixedWrapper::from(balance_bp);
        let fxw_balance_tp = FixedWrapper::from(balance_tp);

        // Product of pool pair amounts to get k value.
        let (pool_k, fxw_pool_k) = {
            if empty_pool {
                if abstract_checking {
                    (None, None)
                } else {
                    let fxw_value =
                        to_fixed_wrapper!(init_x).multiply_and_sqrt(&to_fixed_wrapper!(init_y));
                    let value = to_balance!(fxw_value.clone());
                    (Some(value), Some(fxw_value))
                }
            } else {
                let fxw_value: FixedWrapper = fxw_balance_bp.multiply_and_sqrt(&fxw_balance_tp);
                let value = to_balance!(fxw_value.clone());
                (Some(value), Some(fxw_value))
            }
        };

        if !abstract_checking {
            if empty_pool {
                match self.destination.amount {
                    Bounds::Desired(k) => {
                        ensure!(
                            k == pool_k.unwrap(),
                            Error::<T>::InvalidDepositLiquidityDestinationAmount
                        );
                    }
                    _ => {
                        self.destination.amount = Bounds::Calculated(pool_k.unwrap());
                    }
                }
            } else {
                match (
                    (self.source.0).amount,
                    (self.source.1).amount,
                    self.destination.amount,
                ) {
                    (ox, oy, Bounds::Desired(destination_k)) => {
                        ensure!(destination_k > 0, Error::<T>::ZeroValueInAmountParameter);
                        let fxw_destination_k = FixedWrapper::from(init_x);
                        let fxw_piece_to_add = fxw_pool_k.unwrap() / fxw_destination_k;
                        let fxw_recom_x = fxw_balance_bp.clone() / fxw_piece_to_add.clone();
                        let fxw_recom_y = fxw_balance_tp.clone() / fxw_piece_to_add.clone();
                        let recom_x = to_balance!(fxw_recom_x);
                        let recom_y = to_balance!(fxw_recom_y);
                        match ox {
                            Bounds::Desired(x) => {
                                if x != recom_x {
                                    Err(Error::<T>::InvalidDepositLiquidityBasicAssetAmount)?
                                }
                            }
                            bounds => {
                                let value = to_balance!(fxw_balance_bp / fxw_piece_to_add.clone());
                                let calc = Bounds::Calculated(value);
                                ensure!(
                                    bounds.meets_the_boundaries(&calc),
                                    Error::<T>::CalculatedValueIsNotMeetsRequiredBoundaries
                                );
                                (self.source.0).amount = calc;
                            }
                        }
                        match oy {
                            Bounds::Desired(y) => {
                                if y != recom_y {
                                    Err(Error::<T>::InvalidDepositLiquidityTargetAssetAmount)?
                                }
                            }
                            bounds => {
                                let value = to_balance!(fxw_balance_tp / fxw_piece_to_add);
                                let calc = Bounds::Calculated(value);
                                ensure!(
                                    bounds.meets_the_boundaries(&calc),
                                    Error::<T>::CalculatedValueIsNotMeetsRequiredBoundaries
                                );
                                (self.source.1).amount = calc;
                            }
                        }
                    }
                    (
                        Bounds::RangeFromDesiredToMin(xdes, xmin),
                        Bounds::RangeFromDesiredToMin(ydes, ymin),
                        dest_amount,
                    ) => {
                        ensure!(
                            xdes >= xmin && ydes >= ymin,
                            Error::<T>::RangeValuesIsInvalid
                        );

                        let total_iss = assets::Pallet::<T>::total_issuance(&repr_k_asset_id)?;

                        let (calc_xdes, calc_ydes, calc_marker) =
                            Pallet::<T>::calc_deposit_liquidity_1(
                                total_iss, balance_bp, balance_tp, xdes, ydes, xmin, ymin,
                            )?;

                        self.source.0.amount = Bounds::Calculated(calc_xdes);
                        self.source.1.amount = Bounds::Calculated(calc_ydes);

                        match dest_amount {
                            Bounds::Desired(_) => {
                                return Err(Error::<T>::ThisCaseIsNotSupported.into());
                            }
                            _ => {
                                self.destination.amount = Bounds::Calculated(calc_marker);
                            }
                        }
                    }
                    // Case then no amount is specified (or something needed is not specified),
                    // impossible to decide any amounts.
                    (_, _, _) => {
                        Err(Error::<T>::ImpossibleToDecideDepositLiquidityAmounts)?;
                    }
                }
            }
        }

        // Recommended minimum liquidity, will be used if not specified or for checking if specified.
        let recom_min_liquidity = MIN_LIQUIDITY;
        // Set recommended or check that `min_liquidity` is correct.
        match self.min_liquidity {
            // Just set it here if it not specified, this is usual case.
            None => {
                self.min_liquidity = Some(recom_min_liquidity);
            }
            // Case with source user `min_liquidity` is set, checking that it is not smaller.
            Some(min_liquidity) => {
                if min_liquidity < recom_min_liquidity {
                    Err(Error::<T>::PairSwapActionMinimumLiquidityIsSmallerThanRecommended)?
                }
            }
        }

        //TODO: for abstract_checking, check that is enough liquidity in pool.
        if !abstract_checking {
            // Get required values, now it is always Some, it is safe to unwrap().
            let min_liquidity = self.min_liquidity.unwrap();
            let base_amount = (self.source.0).amount.unwrap();
            let target_amount = (self.source.1).amount.unwrap();
            let destination_amount = self.destination.amount.unwrap();
            // Checking by minimum liquidity.
            if min_liquidity > pool_k.unwrap()
                && destination_amount < min_liquidity - pool_k.unwrap()
            {
                Err(Error::<T>::DestinationAmountOfLiquidityIsNotLargeEnough)?;
            }
            // Checking that balances if correct and large enough for amounts.
            if balance_bs.unwrap() < base_amount {
                Err(Error::<T>::SourceBaseAmountIsNotLargeEnough)?;
            }
            if balance_ts.unwrap() < target_amount {
                Err(Error::<T>::TargetBaseAmountIsNotLargeEnough)?;
            }
        }

        if empty_pool {
            // Previous checks guarantee that unwrap and subtraction are safe.
            self.destination.amount =
                Bounds::Calculated(self.destination.amount.unwrap() - self.min_liquidity.unwrap());
        }

        Ok(())
    }
    fn instant_auto_claim_used(&self) -> bool {
        true
    }
    fn triggered_auto_claim_used(&self) -> bool {
        false
    }
    fn is_able_to_claim(&self) -> bool {
        true
    }
}

impl<T: Config> common::SwapAction<AccountIdOf<T>, TechAccountIdOf<T>, T>
    for DepositLiquidityAction<
        AssetIdOf<T>,
        TechAssetIdOf<T>,
        Balance,
        AccountIdOf<T>,
        TechAccountIdOf<T>,
    >
{
    fn reserve(&self, source: &AccountIdOf<T>) -> dispatch::DispatchResult {
        ensure!(
            Some(source) == self.client_account.as_ref(),
            Error::<T>::SourceAndClientAccountDoNotMatchAsEqual
        );
        let asset_repr = Into::<AssetIdOf<T>>::into(self.destination.asset);
        let pool_account_repr_sys =
            technical::Pallet::<T>::tech_account_id_to_account_id(&self.pool_account)?;
        technical::Pallet::<T>::transfer_in(
            &(self.source.0).asset,
            &source,
            &self.pool_account,
            (self.source.0).amount.unwrap(),
        )?;
        technical::Pallet::<T>::transfer_in(
            &(self.source.1).asset,
            &source,
            &self.pool_account,
            (self.source.1).amount.unwrap(),
        )?;
        assets::Pallet::<T>::mint_to(
            &asset_repr,
            &pool_account_repr_sys,
            self.receiver_account.as_ref().unwrap(),
            self.destination.amount.unwrap(),
        )?;
        let pool_account_repr_sys =
            technical::Pallet::<T>::tech_account_id_to_account_id(&self.pool_account)?;
        let balance_a =
            <assets::Pallet<T>>::free_balance(&(self.source.0).asset, &pool_account_repr_sys)?;
        let balance_b =
            <assets::Pallet<T>>::free_balance(&(self.source.1).asset, &pool_account_repr_sys)?;
        Pallet::<T>::update_reserves(
            &(self.source.0).asset,
            &(self.source.1).asset,
            (&balance_a, &balance_b),
        );
        Ok(())
    }
    fn claim(&self, _source: &AccountIdOf<T>) -> bool {
        true
    }
    fn weight(&self) -> Weight {
        unimplemented!()
    }
    fn cancel(&self, _source: &AccountIdOf<T>) {
        unimplemented!()
    }
}
