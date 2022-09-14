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
#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

// #[cfg(feature = "runtime-benchmarks")]
// mod benchmarking;

#[frame_support::pallet]
pub mod pallet {
	use common::primitives::AssetId;
	use frame_support::{dispatch::DispatchResultWithPostInfo, pallet_prelude::*};
	use frame_system::pallet_prelude::*;
	use xcm::opaque::latest::{AssetId::Concrete, Fungibility::Fungible};
	use xcm::v1::{MultiAsset, MultiLocation};

	#[pallet::config]
	pub trait Config: frame_system::Config {
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	#[pallet::without_storage_info]
	pub struct Pallet<T>(_);

	#[pallet::storage]
	#[pallet::getter(fn get_multilocation_from_asset_id)]
	pub type AssetIdToMultilocation<T: Config> =
		StorageMap<_, Blake2_256, AssetId, MultiLocation, OptionQuery>;

	#[pallet::storage]
	#[pallet::getter(fn get_asset_id_from_multilocation)]
	pub type MultilocationToAssetId<T: Config> =
		StorageMap<_, Blake2_256, MultiLocation, AssetId, OptionQuery>;

	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		MappingCreated(AssetId, MultiLocation),
		MappingChanged(AssetId, MultiLocation),
		MappingDeleted(AssetId, MultiLocation),
	}

	#[pallet::error]
	pub enum Error<T> {
		MappingAlreadyExists,
		MappingNotExist,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {}

	#[pallet::call]
	impl<T: Config> Pallet<T> {
		#[pallet::weight(0)]
		#[frame_support::transactional]
		pub fn register_mapping(
			origin: OriginFor<T>,
			asset_id: AssetId,
			multilocation: MultiLocation,
		) -> DispatchResultWithPostInfo {
			let _ = ensure_root(origin)?;
			ensure!(
				AssetIdToMultilocation::<T>::get(asset_id).is_none()
					|| MultilocationToAssetId::<T>::get(multilocation.clone()).is_none(),
				Error::<T>::MappingAlreadyExists
			);
			AssetIdToMultilocation::<T>::insert(asset_id, multilocation.clone());
			MultilocationToAssetId::<T>::insert(multilocation.clone(), asset_id);
			Self::deposit_event(Event::<T>::MappingCreated(asset_id, multilocation));
			Ok(().into())
		}

		#[pallet::weight(0)]
		#[frame_support::transactional]
		pub fn change_mapping(
			origin: OriginFor<T>,
			asset_id: AssetId,
			multilocation: MultiLocation,
		) -> DispatchResultWithPostInfo {
			let _ = ensure_root(origin)?;
			AssetIdToMultilocation::<T>::try_mutate(asset_id, |ml_opt| -> DispatchResult {
				ensure!(ml_opt.is_some(), Error::<T>::MappingNotExist);
				*ml_opt = Some(multilocation.clone());
				Ok(())
			})?;
			MultilocationToAssetId::<T>::try_mutate(
				multilocation.clone(),
				|asset_opt| -> DispatchResult {
					ensure!(asset_opt.is_some(), Error::<T>::MappingNotExist);
					*asset_opt = Some(asset_id);
					Ok(())
				},
			)?;
			Self::deposit_event(Event::<T>::MappingChanged(asset_id, multilocation));
			Ok(().into())
		}

		#[pallet::weight(0)]
		#[frame_support::transactional]
		pub fn delete_mapping(
			origin: OriginFor<T>,
			asset_id: AssetId,
			multilocation: MultiLocation,
		) -> DispatchResultWithPostInfo {
			let _ = ensure_root(origin)?;
			ensure!(
				AssetIdToMultilocation::<T>::get(asset_id).is_some()
					|| MultilocationToAssetId::<T>::get(multilocation.clone()).is_some(),
				Error::<T>::MappingNotExist
			);
			AssetIdToMultilocation::<T>::remove(asset_id);
			MultilocationToAssetId::<T>::remove(multilocation.clone());
			Self::deposit_event(Event::<T>::MappingDeleted(asset_id, multilocation));
			Ok(().into())
		}
	}

	impl<T: Config> sp_runtime::traits::Convert<AssetId, Option<MultiLocation>> for Pallet<T> {
		fn convert(id: AssetId) -> Option<MultiLocation> {
			Pallet::<T>::get_multilocation_from_asset_id(id)
		}
	}

	impl<T: Config> sp_runtime::traits::Convert<MultiLocation, Option<AssetId>> for Pallet<T> {
		fn convert(multilocation: MultiLocation) -> Option<AssetId> {
			Pallet::<T>::get_asset_id_from_multilocation(multilocation)
		}
	}

	impl<T: Config> sp_runtime::traits::Convert<MultiAsset, Option<AssetId>> for Pallet<T> {
		fn convert(a: MultiAsset) -> Option<AssetId> {
			if let MultiAsset { fun: Fungible(_), id: Concrete(id) } = a {
				Self::convert(id)
			} else {
				Option::None
			}
		}
	}
}
