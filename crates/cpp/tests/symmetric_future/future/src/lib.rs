mod future_world;

use future_world::test::test::future_source::create;
use wit_bindgen_symmetric_rt::async_support;

future_world::export!(MyStruct with_types_in future_world);

struct MyStruct;

impl future_world::exports::test::test::future_test::Guest for MyStruct {
    async fn create() -> async_support::FutureReader<u32> {
        let (write, read) = async_support::future_support::new_future();
        let input = create().await;
        async_support::spawn(async move {
            let input = input.await.unwrap();
            write.write(input * 2).await;
        });
        read
    }
}
